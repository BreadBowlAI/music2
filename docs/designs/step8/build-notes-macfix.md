# Macfix: build notes

The owner's `ci/check.sh` on the reference Mac at `85b069a` failed 2 of 13 rows. This track fixes both. Branch
`macfix`, worktree `~/m2-runs/wt/mf`, from `main` at `85b069a`. Linux timings here are information only (PLAT-1):
only the Mac judges them. Other tracks loaded this server the whole time, so every timing is paired.

## In short

| Row | What failed on the Mac | The cause, as measured | The fix |
|---|---|---|---|
| gates, G7f store | "block within S + 80 µs": +135 µs | **Not pinned.** Not the channel gains: p50 had its 50 `std:gain` before 8b.3, and S plays them too. Linux's "+60 since `22bea45`" didn't come back in paired runs today. Untested candidates for the Mac: std's rebuild, and 8a's block path (partitions, the FP mode) | `std:gain` skips `powf` while the dB holds still: 5.3 µs a node a block at -12 dB becomes 0.46, the same bits. G7f store on Linux: +119 and +22 µs over S before, -148 and -360 after. It speeds up the app's side only, so the Mac also plays the old gain once (below) |
| rust tests | `new_songs_go_where_settings_say` | A Mac's temporary folder is a link (`/var` is `/private/var`). A song opens by its real path; the songs folder kept the link's spelling | The songs folder and the home folder are named by their real paths, as songs are |

Every render keeps its bits: `e2e/renders.sha256`, `renders.json`, and G7f's "same music" hash (`22cd1b3810530499`,
the Mac's own). No std version bump: no render changed (FMT-3). `std.wasm` changed, so every kernel's hash changed
(PLAT-5).

## A. G7f store

### In short

- **The lead's guess was half right.** `std:gain` is p50's biggest avoidable cost: 50 channel gains at -12 and
  -18 dB, each a `powf` on every sample. But p50 always had them, and S plays them too, with the same kernel code.
  So they can't be what moved the line.
- **Linux's "+60" isn't a clear step.** The checks read `a8199ff` -33 µs and `22bea45` +61, one run each. Run again
  today, paired, `e108e46` read -7 and `22bea45` -20. On this server one run of the line moves by about 100 µs.
- **What is left of it is not pinned.** Only std reaches G7f store between `a8199ff` and `22bea45`. The master's
  two new nodes cost +0.4 µs a block. In one process, `85b069a`'s std read +22 to +95 µs over `a8199ff`'s, and
  swapping only `std.wasm` read +37 to +82. But the A/A control read -52 to +24, so that is near the noise. And
  `22bea45` has the same `std.wasm` as `85b069a` (sha256 `3dd1f634…`), yet in the sweep it read -20 µs, against
  +119 and +22 for `85b069a`'s std on the track's binary. So std's rebuild is a candidate, not shown. I couldn't
  pin it further here (no hardware counters).
- **The Mac's +154 since step 7 can't be split on Linux.** On the Mac it sits in what the app and P1 share: P1
  read +177 over S, the app's layout -46 over P1. Untested candidates, all between `cbac57c` and `85b069a`: std's
  rebuilds (std 0.2 with the `sounds` merge; the master's gain and `std:stereo` with 8b.3), and 8a's block path:
  blocks through partitions (`0fd1b20`) and one floating-point mode on every thread (`2dc89ce`, `7a1d527`). None
  was measured on its own on the Mac. On Linux 8a's tip read +47 and `next` +46, one run each.
- **The fix is still `std:gain`.** It is real work every song does, and it is worth far more than the move: G7f
  store's line on Linux went from +119 and +22 µs over S to -148 and -360, with the same bits.
- **But it would hide the +154.** S plays its own std, with the old gain. So the fast path speeds up the app's
  layout and P1 alike, by about 250 µs a p50 block on Linux. Then the line no longer shows the Mac's shift, and
  any layout or engine cost up to about that much reads as a pass. So the owner's next Mac run plays G7f store
  twice: once as `ci/check.sh` does, and once with `85b069a`'s std (the old gain) on the app's side, as the Linux
  sweep's "old" runs did ("The Mac's next run", below). The second one keeps the +135 in view.

### What was measured, and how

| What | How | Log (`~/m2-runs/logs/`) |
|---|---|---|
| One node's cost a block | `m2 render` (1 thread) of a sine through 200 `std:gain` or `std:scale` in a row, 31 s less 1 s, 6 rounds in rotating turns, old std against new | `mf-nodebench2-1017-201338-*.log` |
| p50's graph with each std | `m2 check --json` of p50, kernels counted | (below) |
| The master's cost | `m2 render` of 50 sines at -12 dB, summed, into the old master or the new one, 10 rounds | `mf-master-1017-210453-*.log` |
| p50 by std, paired | a scratch tool (never committed, kept at `~/m2-runs/smf/stdab.rs`): p50 in one process, one renderer per std folder, in G7f store's own turns (`paired::in_turns`: 375 warm, 11,250 blocks, turns of 625 after 188 not judged) | `mf-stdab1-*`, `mf-stdab2-*`, `mf-stdab3-*` |
| G7f store itself | `m2-gates store --reference 50` with S's runner, the track's binary with each std, and the binaries of `e108e46` and `22bea45` | `mf-g7f-1017-212756-*.log` |

### One node, a block (Linux, load 20 to 30)

| Kernel | 85b069a's std | With the fast path |
|---|---|---|
| `std:gain` at 0 dB | 0.80 µs (0.76 to 1.15) | 0.53 (0.30 to 1.06) |
| `std:gain` at -12 and +12 dB in turn | **5.30** (4.80 to 7.02) | **0.46** (0.32 to 0.78) |
| `std:scale` (a plain multiply) | 0.41 (0.37 to 0.60) | 0.42 (0.39 to 1.01) |

Medians of 6 rounds. At 0 dB `powf(10, 0)` returns early, so the old gain was cheap there. At p50's -12 and -18 dB
it is 13 times a multiply, as the 8a spike saw (4.1 µs).

### p50's graph, and the line's history

p50 gives every channel a dB (`db(-12)`, `db(-18)`). Before 8b.3 std's mixer added a gain when a channel gave a dB,
so p50 had 50 gains then too. `m2 check --json` of p50:

| std | `std:gain` | `std:stereo` | Top-level nodes |
|---|---|---|---|
| `a8199ff` (before 8b.3) | 50 | 0 | 467 |
| `85b069a` | 51 (the master's, at 0 dB) | 1 | 470 |

S (`8aa301e`) plays p50 with its own std, whose mixer and `Gain` are the same code: 50 gains too.

The line on Linux, from earlier `ci/check.sh --gates` runs (one run each):

| Commit | Block over S, by the mean |
|---|---|
| `ffc36aa` (step 7) | +20 µs |
| `e8bba0e` (sounds) | -35, +10 |
| `09b2dfd` | +10, +2 |
| `e108e46` | -25 |
| `a8199ff` (8b's back ends) | -33 |
| `22bea45` (taste-next, 8b.3) | **+61** |
| `bc41693` (8a), `1560a19` (next) | +47, +46 |

Between `a8199ff` and `22bea45`, `core/runtime`, `crates/gates` and p50's code didn't change. `core/play` (not used
by G7f store), `core/script` (editor labels) and `core/session` (an enum) did. And std: the master's gain and
`std:stereo`, and `std.wasm` rebuilt with the new kernel.

### Which part of std's change

The master alone: 50 sines at -12 dB, summed, into the old master or the new one: **+0.4 µs a block** (10 rounds;
one disturbed round read +156). Not the cause.

p50 in one process, one renderer per std folder, in G7f's turns, all playing the same bits (`22cd1b3810530499`):

| Run | 85b069a's std over a8199ff's | a8199ff's std with 85b069a's `std.wasm` over a8199ff's | A/A (a8199ff's twice) |
|---|---|---|---|
| 1 (load 3 to 10) | +94.5 µs (-122 to +219) | | +12.3 (-35 to +422) |
| 2 (load 3 to 11) | +21.6 (-205 to +164) | +82.4 (-107 to +266) | -52.1 (-206 to +387) |
| 3 (load 5 to 41, held on CCD1, thread CPU time) | +41.9 (-89 to +590) | +36.7 (-114 to +328) | +23.3 (-90 to +208) |

Medians of 5 or 6 rounds. Swapping only `std.wasm` moves p50 about as much as all of std's change. But the A/A
control moves by up to 52 µs too. So this machine can say only this much: a few tens of µs at most, and it follows
the rebuild, not the master.

The two `std.wasm` builds, function by function: 355 and 361 functions (`std:stereo`'s 6 are new). Every kernel's
`_init` and `_process` has the same size in both; the bytes differ only in constants (call numbers and the
addresses of statics, which a new kernel shifts). So no kernel does more work. What moved is where code and
statics sit. Such shifts are known to move timings by a few percent, either way. Telling which shift would need
hardware counters (`perf` isn't here, and `perf_event_paranoid` is 4) or the Mac.

### The fix: `std:gain`'s fast path

`std/kernels-src/src/lib.rs`:

- `Gain` keeps the last dB it read and its gain. `db_to_gain` (a `powf`) runs only when the dB changes. It starts
  as NaN with NaN's own gain, so the kept pair is always right.
- `Smooth::settled(target)`: a step toward `target` gives back the value the smoother holds, bit for bit. The
  smoother can stop a little short of its target (a step rounds back to where it was), so this checks the step
  itself, not "close to the target".
- `Gain::steady(db)`: when the dB is the same, bit for bit, all block, and the smoother has settled on its gain,
  that gain is every sample's. Then the block is `x * g`, the same multiply, and the smoother's state stays
  exactly as the per-sample loop leaves it. Else the per-sample loop runs as before, with the kept gain.
- Mono leaves the right channel at 0, as before.

No sound changes, so std stays 0.2.0 (FMT-3 bumps only when a render's bits change; 8b.3's `std:stereo` didn't bump
either). `std.wasm` rebuilt with `std/build.sh`; `std/build.sh --check` rebuilds it to the same bytes in another
folder. Its sha256 was `3dd1f634…`, now `699670b4…`: every kernel's hash changed (PLAT-5).

How G7f's S gets its std: its runner plays S's own std (`crates/gates/s/README.md`), whatever `M2_STD` says. So the
fix speeds up the app's side only: the app's layout and P1, not S. The line then says the app beats S, by about the
gains' old cost (about 250 µs a p50 block on Linux). That would hide as much of any other cost, so the Mac also runs
the gate with the old gain (below).

### Tests, red first

`std/kernels-src/src/tests.rs` (new): the per-sample loop as it was (`per_sample`), and std:gain, side by side.
Every output sample and the smoother after every block must be the same bits. Each test also says which blocks were
one gain, so the fast path is known to run.

| Test | What it plays |
|---|---|
| `zero_db_is_one_gain_from_the_second_block` | 0 dB for 2 s: every block but the first is one gain |
| `a_settled_value_is_one_gain` | -6 dB |
| `steps_at_a_block_edge_and_inside_a_block` | 0, -12 (at a block's edge), +6 (inside a block), -120 (silence), 0 |
| `ramps_and_automation` | a ramp moving every sample (never one gain), a hold, a wobble, a hold |
| `a_block_whose_db_changes_at_one_sample` | a settled -6 dB; one block's last sample, then another's first, at -6.5 |
| `any_block_size_mono_or_stereo` | blocks of 1, 37, 0, 128, 64, 3 and 100 samples, mono and stereo |
| `odd_db_values` | -0 dB beside 0 dB, exactly -96 dB and -95.99, infinities, NaN |
| `played_through_its_ports_it_gives_the_same_bits` | the kernel through its ports (`process`), 1 and 2 channels, in blocks of 1, 37 and 128, against the old loop over the whole signal |

The audio has -0.0, a subnormal, a huge value, infinity and NaN in it, so any other multiply would show.

Red: each mutant of the fast path below fails (all 8 pass on the fix).

| Mutant | Fails |
|---|---|
| multiply by the dB's gain, not the smoother's value | 6 tests, at the first sample that differs |
| look at the block's first dB only | 6 tests |
| "settled" as within 1e-6 of the target | 4 tests: never one gain |
| "settled" as within 1e-4 of the target | 4 tests, on bits (`-0.33237264 against -0.33237273`) |
| a mono block leaves its right channel as it was | 1 test |

### Numbers after the fix (Linux, information)

p50 in one process (run 1 above): the fast path's std against a8199ff's: **-178 µs a block** (-367 to -165), and
the same bits.

### The G7f sweep (Linux, information only)

`m2-gates store --reference 50`, S's runner built from a clean copy of `8aa301e` (`~/m2-runs/smf/s-8aa301e`), one
run after another (`~/m2-runs/smf/g7f-sweep.sh`, log `mf-g7f-1017-212756-3182255.log`). "old" and "new" are the
track's binary with `85b069a`'s std and with the fast path's std (`M2_STD`).

| Run | Start, load | Block over S, by the mean | P1 over S | The app's layout over P1 |
|---|---|---|---|---|
| old | 21:28, 4 | +119 µs (5 pairs, -5 to +165) | +25 | +17 |
| new | 21:36, 5 | **-148** (-190 to -38) | -177 | +1 |
| `e108e46` | 21:43, 1 | -7 (-39 to +52) | -7 | -3 |
| `22bea45` | 21:50, 3 | -20 (-86 to +211) | +49 | -59 |
| new | 22:00, 8 | **-360** (-705 to +12) | -284 | -9 |
| old | 22:09, 13 | +22 (-316 to +188) | -10 | -22 |

Every run: "the same music", hash `22cd1b3810530499` (2,880,000 samples), the app's layout, P1 and S in every round:
the Mac's own hash. New against old: -267 and -382 µs, about the 50 gains' old cost (50 × 5.3 µs).

On the Mac (expected, not measured): its gains cost about as much, so its +135 should land far under the bound.
The owner's next `ci/check.sh` says. That pass would come from the gains, not the layout, so it can't say whether
the +135 went away.

### The Mac's next run

Once macfix is on `main`, the owner runs `ci/check.sh` on the Mac, then this, in the same Terminal (about 10
minutes; S's runner is already built in `~/m2-check/s-8aa301e`):

```sh
cd ~/dev/suite2/music2
mkdir -p ~/m2-check/std-85b069a && git archive 85b069a std | tar -x -C ~/m2-check/std-85b069a
export M2_GATES_S=$(crates/gates/s/build.sh | tail -1)
M2_STD=~/m2-check/std-85b069a/std cargo run --locked --release -p m2-gates -- store --reference 50 2>&1 | tee ~/m2-g7f-old-gain.txt
```

It plays G7f store with the app's side on `85b069a`'s std, the old gain, against S as always. What it says:

| "block within S + 80 µs" with the old gain | Then |
|---|---|
| about +135 again | the Mac's shift is still there, and still unexplained. The check's pass comes from the gains. Next: the candidates above, one at a time on the Mac |
| near step 7's -19 | the shift is gone with the old gain too, so the fast path hides nothing. Write it in RENDER-32; nothing more to chase |
| anything between | run it once more before deciding (one run is good to about ±30 µs a block) |

Tried on Linux (information only), with S's runner built again in `~/m2-runs/smf/s-8aa301e` and the track's
binary (`~/m2-runs/smf/g7f-archive.sh`, log `mf-g7f-archive-1017-225523-*.log`, load 9 to 6, 11 minutes). The
archive's `std.wasm` is `3dd1f634…`, the same folder as the sweep's "old". It played the same music
(`22cd1b3810530499`) and read **-59 µs** over S (5 pairs, -281 to +52); P1 +254 over S, the app's layout -337 over
P1. This server can't say more than that: one run here moves by about 100 µs.

## B. The songs folder

### In short

- **What failed:** `crates/server/tests/songs_folder.rs:38`, "a new song is made there". On a Mac,
  `std::env::temp_dir()` is under `/var`, a link to `/private/var`.
- **Where the paths part:** a song opens by its real path (`Snapshot::read` canonicalizes the folder), so a new
  song's `dir` is `/private/var/…/My Songs/Night`. The songs folder kept the user's spelling, `/var/…/My Songs`.
- **Why it matters to a user:** with a songs folder chosen through a link (say `~/Dropbox` pointing at another disk),
  the New Song dialog said "New songs are saved in ~/Dropbox/Songs" and then "Made Night in /Volumes/…/Songs". One
  folder, two names. Save As, Recent and the title name it the second way.
- **The decision:** every folder the window shows is named by its real path, as songs already are. The songs folder
  (chosen or default) and the home folder (which turns paths into `~/…`) are resolved as far as they exist.
  Settings shows the real place; `settings.json` keeps the folder as chosen, so a link that moves later is
  followed.

### What changed

| File | Change |
|---|---|
| `crates/server/src/project.rs` | `songs_dir()` and `default_songs_dir()` return the folder by its real path (`real`, through the existing `resolved`: links resolved as far as the folder is there, the rest as written). New `home()`: the home folder by its real path. `catalog()` and `save_as_start()` answer with it |
| `crates/server/src/routes_settings.rs` | Settings answers with `project::home()` |
| `crates/server/tests/songs_folder.rs` | HOME and the chosen folder are reached through a link; every answer names them by their real paths |
| `e2e/run.mjs` | The suites' root by its real path, so a run in a Mac's temporary folder names folders as the app does |
| `README.md` | Settings: a folder reached through a link is shown at its real place |

### Red first

| Run | Result |
|---|---|
| The old test, its folder through a link, old code | **FAIL** at `songs_folder.rs:40`, "a new song is made there": `left: "/tmp/m2-songs-folder-3171527/real/My Songs/Night"`, `right: "…/link/My Songs/Night"`. The Mac's failure, on Linux (`mf-red-*.log`) |
| The new test, old code | **FAIL**: "before the settings are read: the default, by its real path": `…/link/home/Music/music2` against `…/real/home/Music/music2` |
| The new test, the fix | pass; `m2-server`'s 45 `project` unit tests pass (`mf-green-*.log`) |

### The review's fix: a test that holds on any HOME (SONGS-REAL-PATH-SHOWN)

The first fix changed `the_songs_folder_is_under_home` to "starts with `home()`, the real home". That fails when
`~/Music` is a link out of the home folder (say to a Music folder on another disk). The songs folder then resolves
to the link's target, and the home folder stays where it is. `ci/check.sh` runs tests with a scratch HOME with no
link, so it never saw it. `cargo test` in a real HOME would. Now the test says what it means: the songs folder is
`$HOME/Music/music2`, by its real path.

| Run (`~/m2-runs/smf/songs-test.sh`: HOME with `Music` a link out of it, then a plain HOME) | Linked `Music` | Plain |
|---|---|---|
| The first fix's test (`mf-songs-red-1017-225111-*.log`, `mf-songs-red2-1017-225234-*.log`) | **FAIL** at `project.rs:1156`, "under the home folder, by its real path as the songs folder is" | pass |
| The test now (`mf-songs-green-1017-225242-*.log`) | pass | pass |

`m2-server`'s clippy `--all-targets -D warnings`: clean (`mf-clippy-1017-225256-*.log`).

Left for the owner: with a linked `~/Music`, Settings and New Song show the real place (say
`/Volumes/Big/Music/music2`), not `~/Music/music2`. That is the decision above: one folder, one name everywhere. If
the owner wants the folder as spelled instead, it would be resolved only to compare (HANDOFF, Open owner decisions).

## C. Checks

All on Linux, in `~/m2-runs/smf/target` (and `target-kernels`, `target-wasm`), HOME a scratch folder, `M2_AUDIO=none`,
`M2_MIDI=none`.

| Check | Result |
|---|---|
| std kernels: `cargo test --locked --release --workspace`, clippy `-D warnings` | 119 passed (8 new); clippy clean |
| `std/build.sh --check` | the same bytes, rebuilt in another folder |
| `m2-server`: clippy `--all-targets -D warnings` | clean |
| Tests of `m2-server`, `m2-session`, `m2-runtime`, `m2-play` (the last three play std) | all passed, none failed (`mf-checks-*.log`). `the_mixer_and_master_gains_change_no_bit` and `fixture_renders_change_only_with_stds_version` among them |
| Renders: `e2e/baseline.sh --tree .` with the debug `m2` | 10 of 10 match `e2e/renders.sha256` |
| Browser suites (debug headless server, `web/` built) | nx-settings 42/42, s8-mix 80/80, pl-mix 47/47, newsong 71/71 |
| The same two suites from a root reached through a link | old `run.mjs`: nx-settings 5 failed, newsong 3 failed (the app names real paths, the suite the link's); new: 42/42, 71/71 |

## Left

- The Mac run, with G7f store played once more on the old gain ("The Mac's next run"). Only it judges G7f, and
  only that second run still shows the +154 since step 7. What it was made of is not pinned: std's rebuilds and 8a's
  block path (partitions, the FP mode) are untested candidates.
- std's other per-sample `powf`s: `std:drive` (17 in p50, 137 µs a block in the 8a spike) runs `db_to_gain` and
  two `tanh`s per sample on a dB that mostly holds still. The same kind of fast path would apply. Not done here: it
  isn't what failed, and S pays it too.
- Copying at exactly 0 dB instead of multiplying by 1.0 was left out on purpose: the two differ only in a NaN's
  bits, and a multiply costs what `std:scale` costs (0.4 µs).
