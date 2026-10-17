# 8b-ui: build notes

What each slice of the `8b-ui` track did, and what its checks said. The track builds the screens for
mute and solo, clips, hearing notes as you draw them (audition) and meters. Its plan is
`~/m2-runs/s8ui/plan.md`. The lead took every open question's default (Q1 to Q24). For Q15 that means:
no DSP pill and no master meter in the top bar; the status pill says "Heavy load" and "Sound dropped
out"; the master meter sits on the Song row.

All runs are on Linux (jerry-ax102), in the worktree `~/m2-runs/wt/ui`, branch `8b-ui`. Each slice
adds its own section here, in the same commit.

## In short

| Step | What the user gets | State |
|---|---|---|
| Setup | Nothing new to see. The back ends of 8b-backend are merged in. A deleted track no longer leaves its mute behind | done |
| U0 | Nothing new to see, except the roll's grid control now comes from a shared file. The places the lanes fill are ready | done |
| U4 (lane B) | The timeline has a grid and snap: a chip in its corner, Auto by default. Dragged clips land on it; hold ⌘ to place one freely. ⌘1 to ⌘4 change the grid of the view you last used | done |

## Setup: the merges

**In short**
- `8b-ui` starts from taste-next (`c988572`) and merges 8b-backend (`41dabc7`, B1 to B6 and B9 to B12).
- Only three files conflicted. In each, both sides had added lines. Both were kept.
- A track deleted with the builder now forgets its mute and solo. A new track under an old key starts
  heard. A new test shows it.
- Checks: 717 Rust tests pass in the seven crates the merge touched. Clippy is clean.

### What was built

| Commit | What |
|---|---|
| `44091ab` | The merge (`rerere` on, so 8b-backend can replay these resolutions when it lands). `core/play/src/lib.rs`: taste's play and stop count beside 8b's live notes and running flag. `docs/architecture.md`: RENDER-29 keeps taste's words and adds 8b.6's sentence; both sides' bullets kept. `docs/gaps.md` #8: Stop All Sound works (taste), audition's back end is built (8b). |
| `aa572bc` | `forget_overlays` wired in (refresh.md Q7), with its test and these notes |

### The `forget_overlays` wiring

| Where | What |
|---|---|
| `Graph::add_track`, `Graph::duplicate_track` | After the edit, the new track's key is forgotten: a mute kept for an old track of that name doesn't mute the new one |
| `Graph::delete_track` | After a delete that went through (200), its key is forgotten. A 409 (other tracks read it) forgets nothing |
| Why in `Graph`, not the routes | The routes and any other caller get it the same way |
| Undo of a delete | The track comes back heard. Listening mutes are not undo steps (design-8b §18 B16) |
| Test | `crates/server/src/graph/forget_tests.rs` (a new file, refresh.md G4): `a_deleted_track_forgets_its_mute_and_a_new_one_starts_heard` |

std.wasm: no conflict. Neither side touched `std/kernels-src`, so it wasn't rebuilt.

### Checks

| Check | Result |
|---|---|
| `cargo build --locked --workspace --all-targets` | ok; `Cargo.lock` unchanged |
| `cargo test --locked -p m2-server -p m2-editor -p m2-session -p m2-play -p m2 -p m2-script -p music2-desktop` | 717 passed, 0 failed (m2-server 208, m2-editor 210 + 3, m2-session 152, m2-script 55 + 8, m2-play 19 + 40, m2 15, music2-desktop 7) |
| `cargo clippy --locked` on the same crates, `--all-targets -- -D warnings` | clean |
| Log | `~/m2-runs/logs/ui-test-1017-060706-268687.log` |

**Red first:** the new test on the merge alone (the wiring taken out, the test kept) fails at its first
check after the delete: hats' mute stays, `["hats", "synth-pluck"]` where `["synth-pluck"]` was
wanted (`~/m2-runs/logs/ui-forget-red-*.log`). With the wiring it passes.

### Left

- A track renamed by key keeps its mute (8b-backend's `rekey`). Nothing to wire.
- The CLI's `m2 add-track` and `m2 delete-track` open their own session: they leave `overlays.json` as
  it was. The app's next add or delete of that key cleans it.

## U0: the seams

**In short**
- Nothing new to see, but one thing moved: the roll's grid and snap control now comes from a shared
  file. The timeline gets the same control in U4.
- The empty places the lanes fill are ready: M and S, the meter, the mutes pill.
- The new keys, chords and menu rows are in. Each does nothing until its lane fills it.
- A muted track already looks muted: its name and clips follow the server's state.
- One change you may spot: a long track name is cut at x = 168, not 204. That leaves room for M, S
  and the meter. The ⋯ now shows just left of M.
- Checks: the web build, nine browser suites with their counts unchanged, the menu test, and a probe
  of the muted looks.

### What changed

| File | What |
|---|---|
| `web/src/lib/events.ts` | The socket can send: `send(msg)` (false when it isn't open; nothing is kept). Listeners by message type: `on(type, fn)`. Listeners of `CLOSED` hear the socket close |
| `web/src/App.tsx` | Toasts, `graph` and `export` come through `on`, as before |
| `web/src/lib/graph8b.ts` (new) | The 8b types, added to `graph.ts`'s by `declare module`: `OverlayState`, `GraphState.overlays`; `live`, `load`, `late`, `xruns`, `device_errors` on the status; `sha` on clips; the audition and meters messages. `useHeard(module)` reads the state: "heard", "muted", "muted-in-song" or "silenced" |
| `web/src/components/TrackHeader.tsx` (new) | The header's content, moved from `SongTimeline.tsx`: the fold chevron, the name, the issue mark, the ⋯. Plus the M and S slot and the meter slot (below). The name's look follows `useHeard` |
| `web/src/components/SongTimeline.tsx` | Uses `TrackHeader`. A muted or silenced track's clips draw at 40% and grey (`grayscale(0.6)`); muted in the song, they are hatched too (`Hatch`, an SVG pattern). The timeline is marked `data-grid-view="song"` |
| `web/src/lib/grid.ts` (new) | The grid, moved out of the roll: `GRIDS`, each view's grid and snap (`song`, `notes`), kept in `localStorage` under the roll's old keys (`m2.notes.*`) and new ones (`m2.song.*`), and which view ⌘1 to ⌘4 act on (`followGridView`: the last one pressed or focused in) |
| `web/src/components/GridChip.tsx` (new) | The grid menu and the snap button, moved out of the roll. Test ids `grid` and `snap` in the roll, `song-grid` and `song-snap` on the timeline (U4) |
| `web/src/components/NoteView.tsx` | Uses `GridChip view="notes"` and the store. Marked `data-grid-view="notes"`. Nothing changes on screen |
| `web/src/components/TopBar.tsx` | The mutes pill's mount, just right of the status pill |
| `MuteSolo.tsx`, `TrackMeter.tsx`, `OverlayPill.tsx` (new) | Empty mounts: each renders nothing until its lane fills it |
| `web/src/lib/shell.ts` | New `canvas` hooks (below) |
| `web/src/lib/native.ts` | `control(key)`: "⌃M" on a Mac, "" elsewhere |
| `web/src/components/CodeView.tsx` | In the code editor, ⌘E and ⌘J stay the editor's (find with the selection, join lines) |
| `apps/music2-desktop/src/menu.rs` | Track: Mute (⌃M, Mac only), Solo (⌃S, Mac only), Mute in Song. View: Narrower Grid ⌘1, Wider Grid ⌘2, Triplet Grid ⌘3, Snap to Grid ⌘4. The tests pass as they were |
| `crates/server/examples/headless.rs` | `M2_WEB=<folder>` serves that folder's page: one server build serves any lane's `web/dist` |

### Where the header's parts sit

Counted from inside the header's 1 px right border, so at 240 px they land where taste.md §3.4 says.
Measured by the probe:

| Part | At 240 px | At 200 px (a window under 1280 px) |
|---|---|---|
| Name | x = 28 to 168 (to 152 while the ⋯ shows) | x = 28 to 128 |
| ⋯ (on hover) | x = 156, 20 px | x = 116 |
| M and S slot | x = 180 to 222 (M 180, S 202, 20 px each) | x = 140 to 182 |
| Meter slot | x = 231, 3 × 28 px | x = 191 |
| Song row | the name to x = 219, the meter slot at 231; no M or S | the meter at 191 |

### For the lanes: what to fill

| Lane | Fill | How it is reached |
|---|---|---|
| A (U1, U2) | `MuteSolo.tsx` (`{module, name}`) | its slot in every track header |
| A (U1) | `OverlayPill.tsx` (`{graph}`) | the top bar, after the status pill |
| A (U1, U2) | `canvas.mute`, `canvas.solo`, `canvas.muteInSong` (each gets the track's key) | the M and S keys; Track ▸ Mute, Solo, Mute in Song; ⌃M and ⌃S on a Mac page. Only for a selected track, never the Song row |
| B (U4) | `canvas.grid(op)`, op `"narrower"`, `"wider"`, `"triplet"` or `"snap"`; act on `useGrid.getState().last` | ⌘1 to ⌘4; View's grid rows |
| B (U4) | `<GridChip view="song" />` in the timeline's corner | — |
| B (U6) | `canvas.split`, `canvas.join` | ⌘E, ⌘J (Edit rows come in U6). In the code editor they stay its own |
| B (U6) | `canvas.quantize` | `appCommand("quantize")` (the Edit row comes in U6; Q is the timeline's own key) |
| B (U6) | `canvas.takesClipboard()` and `canvas.clipboard(op)` | in a browser, ⌘C ⌘X ⌘V go to `clipboard` only while `takesClipboard()` says so and no field has focus. In the app, U6's `beforecopy` handlers |
| C (U10) | `TrackMeter.tsx` (`{module, name}`; `module` is `song` on the Song row: the master) | its slot in every track header and the Song row |
| C (U9, U10) | `on("audition", …)`, `on("meters", …)`, `on(CLOSED, …)`, `send({t: "audition", …})` | `web/src/lib/events.ts` |

Every lane: `useHeard(module)` is real now, and the types are in `graph8b.ts`.

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `taste` (the lint) | 39 of 39 |
| `pc-keys`, `s8-tracks`, `layout`, `views` | 37, 75, 39, 30: all pass |
| `notes`, `fixes-notes`, `gestures`, `s4-clips` | 52, 37, 30, 28: all pass |
| `cargo test --locked -p music2-desktop` | 7 passed (the menu tests unchanged) |
| A probe, not a suite (first-song with `overlays.json` = Kick muted, Bass soloed, and Hats muted in the song by `m2 mute-in-song`), served with `M2_WEB` | Kick's name `fg-muted`, its clips 0.4 and `grayscale(0.6)`. Hats struck through and hatched. Keys `fg-faint` (silenced by the Bass solo). Bass and the Song row as before. The slots where the table above says, at 1440 and 1100 px. No page errors |
| Logs | `~/m2-runs/logs/ui-u0-suites2-*.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/u0` |

**Red first:** no new suite: U0 adds no behaviour a user can reach. The suites ran before and after
the 1 px fix of the slots (both green); the probe showed the slots 1 px left of taste.md's x before it.

### Left

- The ⌥K sheet's rows: each lane adds its own (Tracks, Edit, View groups).
- Q and the arrows on clips are the timeline's own keys (lane B, U6), not page keys.
- The grid's Auto step, triplets and the timeline's chip: U4. Until then the song grid's start is a
  beat, as the timeline's drag is today.
- Settings that reset at each app launch (the grid, later the audition toggle): one line in
  `docs/gaps.md` at the End.

## U4: the grid and snap on the timeline (lane B)

**In short**
- The timeline has its own grid now, like Notes has. A small chip sits in its top-left corner,
  between **+ Track** and the fold arrow. It says **Auto** at first.
- Auto picks the finest grid whose lines are at least 12 px apart. At today's zoom that is 1/8: a
  clip you drag lands on an eighth note.
- Hold ⌘ (Ctrl on Linux) while you drag to put a clip anywhere. The magnet button, or ⌘4, turns
  snap off.
- ⌘1 makes the grid finer, ⌘2 coarser, ⌘3 turns triplets on or off. They change the grid of the
  view you last clicked in: the timeline or Notes.
- A click on empty lane space still moves where play starts, now on the grid too. It writes nothing.
- Checks: the web build, the new `s8-clips` suite (22 checks), and 9 suites it could move.

**The lane's merge.** Before U4, lane B merged `8b-backend` at `bc761ef` (B7, B8) into `8b-ui-b`
(`5a34189`), with no conflict, for U5. Its server is built in `wt/ui-b` and copied to
`~/m2-runs/s8ui/bin/ui-b-5a34189/`. Cargo.lock didn't change.

### What changed

| File | What |
|---|---|
| `web/src/lib/grid.ts` | A grid is now a choice (`pick`: a step, `bar` or `auto`), triplets and snap. The timeline's choices: Auto, Bar, 1/2, 1/4, 1/8, 1/16, 1/32, 1/64; the roll's: 1/4 to 1/64 (taste.md §3.5). `stepOf` works out Auto from the zoom (finest of bar, beat, 1/8, 1/16, 1/32 with lines 12 px apart, design-8b §7.3). `snapTo` lands on the nearest line, counted from each bar line, so a triplet grid in 3/4 still meets every bar. `snapSong` is the timeline's: anywhere while snap is off or ⌘ is held. `gridOp`: ⌘1 to ⌘4. The roll's `step` is still a plain number of beats, so `NoteView.tsx` (lane C's) didn't change |
| `web/src/components/GridChip.tsx` | The menu lists the view's choices, then **Triplets ⌘3**. The label says "1/8T" in triplets. The tooltips say what Auto is now and the four keys, in the platform's words |
| `web/src/components/SongTimeline.tsx` | The chip in the corner (`song-grid`, `song-snap`). A dragged clip's start lands on the grid (it was whole beats). A click on empty lane space lands on the grid too (it was whole beats). `canvas.grid` acts on the view last used. The lanes draw the grid's own lines in `hover` where it is finer than a beat and its lines are at least 12 px apart |
| `web/src/components/Shortcuts.tsx` | View: Narrower / wider grid ⌘1 / ⌘2, Triplet grid ⌘3, Snap on / off ⌘4, "hold ⌘ while dragging" |
| `e2e/s8-clips.mjs` (new), `e2e/run.mjs`, `e2e/README.md` | The suite, its row after `s4-clips` |

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `s8-clips` (new) | 22 of 22 |
| `taste`, `notes`, `s4-clips`, `s8-songbar` | 39, 52, 28, 68: all pass |
| `layout`, `views`, `pc-keys`, `s8-tracks`, `gestures` | 39, 30, 37, 75, 30: all pass |
| Server | `~/m2-runs/s8ui/bin/ui-b-5a34189` (lane B's build, with B7 and B8), serving `web/dist` through `M2_WEB` |
| Logs | `~/m2-runs/logs/ui-b-u4-suites-*.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/ui-b-u4` |

**Red first:** `s8-clips` on U0's page (the same server): no chip, no eighth lines, a drag of 32 px
landed on beat 1 (whole beats), and holding ⌘ still snapped (beat 2). 1 check passed and 6 failed
before it stopped at the missing chip (`~/m2-runs/s8ui/scratch/e2e/ui-b-u4-red/s8-clips-fix.log`).

### Choices

| Choice | Why |
|---|---|
| Triplets are a row in the grid menu, not a third button | The corner is 200 px wide in a small window. The label shows it: "1/8T" |
| ⌘1 or ⌘2 from Auto gives the fixed step next to what Auto is now | Auto at 1/8: ⌘1 gives 1/16, ⌘2 gives 1/4. The menu brings Auto back |
| The ruler's click and loop drag stay in whole beats | taste-next built them; s8-songbar checks them |

### Left

- Notes doesn't skip the snap with ⌘ held: that is `NoteView.tsx`, lane C's file. The roll does
  get Triplets and 1/64 from the shared menu, and ⌘3 works there.
- The grid resets at each app launch (Q7): the line in `docs/gaps.md` comes at the End.
- The drag's value bubble and guide line come with U5.

