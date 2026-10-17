# Taste's next part: build notes

What each slice of taste's next part did, and what its checks said. The plan is
`~/m2-runs/taste/plan.md`, with its Attack and the lead decisions T1 to T8, which override it. Each slice
adds its own section. All runs are on Linux (jerry-ax102), in the worktree `~/m2-runs/wt/tn`, branch
`taste-next`, made from `09b2dfd`: `main` (`cbac57c`, step 7) plus the `sounds` merge's slices 1 and 2.
The `sounds` merge's slices 3 (std 0.1 to 0.2) and 4 (docs) are not in it yet.

The owner asked for speed as well as quality. So each slice runs the tests of the crates it touched
and the browser suites it can affect. The full `ci/check.sh` runs at the end of step A (after A3) and
in the final phase.

## A1: the merge of `a0d5f91`

**In short**
- Taste's new look and 8b.1 Tracks are merged onto step 7 and `sounds`. The 10 conflicts were the ones
  the plan named, and each was resolved as it says.
- + Track now offers `sounds`' 22 instruments, in three groups: Drums, Instruments, and Sound design
  (the two with every knob). std's first five instruments leave the list (lead decision T3).
- "Paused for export" and "Loading …" are now states of the status pill, below every problem (T2).
- The Mac app's top bar is checked too: a long name ran over the tempo there. Fixed.
- An export's notes and its done toast are one toast now, so Show in Finder stays.
- The watcher looks at `layout.json` only when an event names it. An idle project is again watched
  with no stat.
- A new live test: adding and deleting a track keeps a ringing tail.
- Checks: every crate touched passes; clippy is clean; all 32 browser suites pass, live3b with the test
  plugins; 10 of 10 renders match. No render record moved.

### Slice 0

| What | Result |
|---|---|
| `git merge-tree --write-tree 09b2dfd a0d5f91` | 10 conflicts, the ones the task names: `delta.rs`, `delta_tests.rs`, `graph.rs`, `graph/tests.rs`, `architecture.md`, `gaps.md`, `e2e/README.md`, `fx-fixes.mjs`, `instruments.ts`, `GraphCanvas.tsx` |
| `sounds`' L3 (a `time` node per component call, placed under its frame chain) | In the base (`02aab27`), so break 1's test meets it |

### What was built

| Commit | What |
|---|---|
| `d5a4a1f` | The merge. Only the resolutions of plan §2.2: `delta.rs` keeps step 7's `scope(w, …)` and adds taste's `within`; both sides' tests and `Inner` fields kept; WRITE-17 is taste's text with the platform step's network-volume words; `gaps.md`'s top three from both; `instruments.ts` has taste's `category` and `sounds`' `description`; `GraphCanvas.tsx` is taste's. |
| `f98fdba` | Break 2 (T2, attack C, break 18). The engine's states in the status pill; the playhead holds still while an export holds playback; the Mac app's bar (below). fx-fixes: 75 checks. live3b reads the pill. |
| `ad3b1d9` | Break 3. An export's notes follow the file in its one done toast. |
| `cf3115b` | Break 4 (D7). attention waits until the page has the outside edit (its Undo names nothing) before a toast's Undo. 57 checks. |
| `c03ac4b` | Break 1 (T3, breaks 16 and 17). + Track and `sounds`' instruments (below). |
| `028b442` | Break 19 (and 8). `layout.json` watched by its events. |
| `90035ac`, `2b4c273` | Break 10. The live test of a ringing tail (the second commit only satisfies clippy). |
| `7d5cd3a` | README: a Mac, or Linux (Ubuntu 24.04), with OS-66's packages; the status line no longer says tracks are added in code. |
| `06b30bc` | `e2e/README.md`: live3b's and s8-tracks' rows. |
| `94ea671` | Break 14. views: a window starts on the top track. |

### Decisions made in the slice

| What | Decided | Why |
|---|---|---|
| How + Track leaves out std's first five | The list holds an instrument only when it has a category. The five lose theirs. | PKG-7's text already said a category lists one. A project's own instrument needs a category to be listed now; PKG-7 says so. |
| How "the two every-knob ones last" is done | A third category, Sound design, which the list puts after Drums and Instruments | It uses PKG-7's free string and the list's existing rule (unknown categories after the two). No new SDK field. The design's two groups stay first. |
| Their labels | Synth becomes **Polysynth**, FM becomes **FM synth** | "Synth" and "Synth pad", "FM" and "FM keys" differ by one word. |
| "No two rows that differ by one word or letter" | Read as: no label is another with a word added or taken away, and no two differ by one letter | Those are the attack's pairs (Kick and Kick drum, Hat and Hats, Pad and Synth pad). "Synth pad" and "Synth pluck" stay: their second words differ. A test holds the rule. |
| The order inside a group | Where the package declares them | It gives T3's order without moving code in `sounds`' files: Drum kit, the single drums, FM metal; FM keys, FM bell, Supersaw, Lead, Synth pad, Synth pluck, 808 sub, Synth bass, Strings; FM synth, Polysynth. |
| The Mac app's bar | There the time shows from 1280 px, not 1180, and from 1280 px the name keeps 548 px clear, not 480 | The app's bar starts 80 px in. Measured: its left side ends at 496 px once the time shows, so a long name crossed the tempo at every width. With the time hidden it ends at 432. The bar styles itself from `data-app="mac"` on the root (`main.tsx`), so fx-fixes lays a page out as the app's, with no test hook. taste.md §3.8 says so. |
| fx-fixes' widths | 1100, 1180, 1280 and 1440 px, both bars | **The HANDOFF's "960" becomes 1100**: taste's smallest window is 1100 × 700, so no app window is 960 px wide. |
| The pill's popover | "Paused for export: the export is using the song's plugins. It plays on from here when the export is done." "Loading plugins: <names>. The song plays on meanwhile, and your change joins it once they're ready." | Plain words, one line each. A long plugin list is cut short in the pill; its title has it whole. |
| views' "for the selected module (Bass)" | Not a flake. Fixed in the suite. | Since 8b.1 tracks show in the mixer's order (D8), so a window starts on Kick. It failed every run. |

### Break 1: + Track and `sounds`' instruments

- `tracks::instruments` takes a plain export when there is one, else a member of an exported object.
  A track file then imports the object and calls the member:
  `import { synth } from "std/synth";` and `synth.pad(lane.notes("notes"))`.
- The 22 got a category in `std/drums.ts`, `fm.ts` and `synth.ts`. Each passes taste's SDK rule:
  std loads, and every test that loads it passes.
- Tests:
  - `every_listed_instrument_adds_confined_and_the_song_sounds_the_same`: all 22 added to first-song,
    each `Confined`, each playing from the lane its group says. Then a 2 s render compares equal by
    value to the one before (the attack's `f32 ==`). `synth.strings`, whose chorus reads `time`, is
    `Confined` with L3.
  - `the_instruments_are_stds_sounds_in_order`: the 22 in order, with groups, lanes and labels. It
    checks that the first five are gone and that no two labels are near.
  - The builder tests, the server's test, the CLI's tests and s8-tracks now use `sounds`' instruments.
    The plan said the server's 5-name test would list 27; with T3 it lists 22.
  - PKG-11 (break 17): every track file's `track(…)` call is in `forms.ts`, which tsc checks.

### Break 19: `layout.json` and WRITE-19

- Before, taste stat'd `layout.json` on every watcher tick, 20 a second, against WRITE-19's "no stat
  in between".
- Now the watcher looks at it when an event names it, or a tick walks everything, and then every
  tick until a change holds still. By walks alone, every tick, as before.
- `an_idle_project_is_watched_without_a_stat` now also checks that `layout.json` isn't stat'd, and
  that an outside edit of it still shows by its event, before the next walk.
- FMT-19's *Today* says so.

### Break 10: a ringing tail

`adding_and_deleting_a_track_keeps_a_ringing_tail` (`crates/server/src/graph/tests.rs`):
- A pad chord plays in the driven player. While its release rings, the test adds a track, then
  deletes the first channel's track. That makes the pad's gain new; it crossfades.
- After each, no step in the output is bigger than twice the tail's own. The tail is as loud as the
  same song's with no edit, within 1 dB.

Measured:

| Edit | Tail's own step | Largest step around the edit | After the edit |
|---|---|---|---|
| Add a track | 0.0129 | 0.0111 | the same level as with no edit, to the last digit |
| Delete the first channel | 0.0044 | 0.0057 | 0.00996 against 0.01003 with no edit |

### Checks

| Check | Result |
|---|---|
| `cargo test -p m2-session`, `-p m2-editor`, `-p m2-server`, `-p m2-script`, `-p m2`, after the merge | all pass: 151, 182 (+3), 171, 53 (+1, +7), and m2's 12 |
| The same after the breaks | all pass. m2-editor 183 (the new test), m2-server 172 (the new test) |
| `cargo clippy --workspace --all-targets -D warnings` | clean (after `2b4c273`) |
| The web build (`npm run build`: tsc, the SDK's check, the forms' tsc, vite) | passes |
| Every browser suite (`run.mjs all`, with the test plugins) | 31 of 32 at `2b4c273`, 1,211 of 1,212 checks; views failed (break 14). After `94ea671`, views 30 of 30 alone. live3b 60 of 60, its plugin part included (break 9): no selector needed fixing beyond the pill. |
| The suites each break touches, alone | fx-fixes 75/75, export 37/37, attention 57/57, s8-tracks 75/75 (twice), views 30/30 |
| The renders (`e2e/baseline.sh --tree .`, debug `m2`) | 10 of 10 match `e2e/renders.sha256` |
| `renders.json`, `std.wasm`, `std/m2pkg.toml`, `Cargo.lock` | unchanged since `09b2dfd`. `std/build.sh --check`: the same bytes. |

### Numbers (Linux, information only, PLAT-1)

| What | Here | Before |
|---|---|---|
| s8-tracks: an outside edit of `layout.json` shows | 87 ms, on inotify, before and after break 19 | 172 ms on taste, before the platform step |
| G11 `watcher`, idle (`m2-gates --server tx-data watcher`, release, load about 13) | 0.12% of a core: 2 walks in 10 s, 196 ticks that looked at nothing | 0.10% (the platform step on Linux) |
| G11 `watcher`, an outside write played | p50 161 ms, p95 239 ms in place; p50 166 ms, p95 204 ms as an atomic save | p50 159 ms, p95 213 ms; p50 166 ms, p95 173 ms |
| G11 `tx-data`, to the first block that plays it, p95 | 8.9 ms idle; 8.4 ms with another writer; 9.3 ms with 5,000 assets | 8.5 ms (platform step); 9.0 to 9.2 ms (step 7's check) |

Both G11 gates pass. The machine was shared (load 12 to 15), so the p95s say little. The full
`--gates` run comes at the end of step A.

### Not in this slice

| Break | Where |
|---|---|
| 5, 24: keys and labels per OS | A3 |
| 6, 7, 11, 12, 15, 20, 21, 22: the WIP song bar's fixes | B1 |
| 13: the mixer knobs in the Device view | B5 |
| 23: "loop turned off" during a hold | B2 |

### Left

- The full `ci/check.sh --gates`: at the end of step A. Nothing here ran the mac lint row; no
  macOS-only code changed.
- HANDOFF: its taste merge-notes rows are done now (WRITE-17, D7, the engine note, s8-tracks on
  inotify, the README). The engine note went into the pill (T2), not under the name. Its known-flake
  row for attention's toast Undo can go. Left for A-end's HANDOFF edit.
- The owner's screenshots (+ Track's list with the 22) come from the app on Xvfb at A-end.
- The Mac run judges the pill's place and the Mac app's bar for real. This slice only lays a page out
  as the app's.
- Scratch: `~/m2-runs/tn-scratch` (the suite runner `suite.sh`, suite logs, renders). Logs:
  `~/m2-runs/logs/tn-a1-*`.
