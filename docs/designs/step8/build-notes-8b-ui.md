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
| U5 (lane B) | Clips by hand: move, copy (⌥-drag), resize from either edge, move to another track, select several, delete. Each is one undo step | done |
| U6 (lane B) | Clip commands: a menu on each clip, ⌘D, ⌘E split, ⌘J join, ⌘C ⌘X ⌘V, Q, the arrow keys, and a box dragged across lanes to pick the clips in some bars | done |
| U7 (lane B) | Automation drawn on the timeline: make a clip, add, move and delete points, paint with B; Remove automation from the lane or from the Device view | done |

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

## U5: clip gestures (lane B)

**In short**
- You can now edit clips on the timeline with the mouse:

  | To | Do |
  |---|---|
  | Move a clip | drag it. It lands on the grid |
  | Copy it | hold ⌥ (Alt on Linux) and drag |
  | Make it shorter or longer | drag its left or right edge |
  | Move it to another track | drag it onto that track's lane |
  | Pick more clips | ⇧-click adds one, ⌘-click (Ctrl) takes one in or out |
  | Move several | pick them, then drag one: they all move |
  | Delete | ⌫ |

- While you drag, a small label shows where the clip lands ("9.3.1"), and the track's name when it
  goes to another track. A dashed line marks the spot. The lane it will land on lights up.
- A lane that can't take the clip says why ("Automation only") and doesn't light up. Letting go
  there does nothing.
- Each edit is one undo step. There is no toast: you see the clip where it went.
- If another window or an agent changed that clip while you held it, the edit is refused and a
  toast says so in plain words.
- Automation clips can be picked, moved, copied, resized and deleted like notes clips now.
- Checks: the web build, `s8-clips` (now 55 checks), and 20 suites that touch clips or lanes.

### What changed

| File | What |
|---|---|
| `web/src/lib/clips.ts` (new) | The gestures (`create`, `move`, `resize`, `delete`) sent to `POST /api/graph/clips`, one undo step each, named for the Undo tooltip ("moving a Hats clip"). A clip is named by its id, or by its start and rank (Q11), always with the `sha` the window drew it with (B7): one changed since is refused with 409 and the server's words. `busy`: a gesture waits until the timeline shows the last edit. `useClipSelection`: the selected clips, across tracks |
| `web/src/components/SongTimeline.tsx` | The timeline's edits are gestures now (no whole-lane writes). A drag moves every selected clip by the grabbed clip's snapped distance; onto the lane under the pointer when they come from one lane and it takes them; ⌥ copies. Either edge (6 px, `ew-resize`) resizes. The value bubble (`clip-bubble`) and the dashed guide (`clip-guide`). The target lane lit with `accent-wash`. A dropped drag stays drawn where it went until the timeline shows the edit (no jump back). ⇧-click, ⌘-click, ⌫, Esc. Automation and audio clips take the same gestures. A new clip gets its id from the server and opens in Notes as before |
| `web/src/components/Shortcuts.tsx` | Edit: "Select more clips / one in or out ⇧-click / ⌘-click", "Copy a clip ⌥-drag" |
| `e2e/s8-clips.mjs` | Part 2 (32 checks) |
| `e2e/notes.mjs` | Its last timeline check said automation clips can't be picked. Now: one picked is deleted by Delete, undo brings it back, and a double-click on the lane still makes no clip. Same count (52) |

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `s8-clips` | 55 of 55, three runs |
| `taste`, `s4-clips`, `notes`, `fixes-notes`, `s8-songbar`, `gestures` | 39, 28, 52, 37, 68, 30 |
| `layout`, `views`, `pc-keys`, `s8-tracks`, `s6-write`, `s4-meter` | 39, 30, 37, 75, 36, 8 |
| `attention`, `g11-play`, `live3a`, `s6-builders`, `s4-problems`, `s4-attn`, `newsong` | 57, 24, 49, 55, 18, 13, 71 |
| Server | `~/m2-runs/s8ui/bin/ui-b-5a34189` (B6, B7, B8) |
| Logs | `~/m2-runs/logs/ui-b-u5-suites2-*.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/ui-b-u5b` |
| Pictures (headless page, not the app) | `~/m2-runs/s8ui/scratch/e2e/ui-b-u5-shot/s8-clips-fix/`: `s8-clips-grid.png` (the grid menu), `s8-clips-across.png` (a Kick clip dragged onto Hats), `s8-clips-select.png` (two clips selected across tracks) |

**Red first:** part 2 on U4's page (the same server): no bubble or guide, the move went out as a
whole-lane write (`/api/graph/lane`), not a gesture, and the clip had no edges; it stopped there
(3 failed; `~/m2-runs/s8ui/scratch/e2e/ui-b-u5-red/s8-clips-fix.log`).

**Found while building:** a clip moved to another lane was taken out of its own lane while dragged,
which dropped the pointer and cancelled the drop. It is now hidden there but kept.

### Choices

| Choice | Why |
|---|---|
| A click on a picked clip keeps the selection until you let go | So a drag moves them all; a plain click then picks only it (as Live and Logic) |
| Several clips go to another lane only when they come from one lane | `move` has one `to`. Clips from several tracks move in time only |
| Letting go over a lane that can't take the clips does nothing | Moving them in time instead would surprise. The bubble said why |
| No toast for a gesture | design-8b §2.4; the Undo tooltip names it |

### Left

- U6 can start: B7 (split, join, quantize) is on 8b-backend and merged here. ⌘D, ⌘C ⌘X ⌘V,
  ⌘E, ⌘J, Q, the arrows, the marquee and the clip menu are U6.
- Until U6, ⌘D with only an automation clip selected duplicates its track (`App.tsx`'s order gets
  the clips step in U6), and ⌘L loops over the clip Notes shows, not over the selection
  (`GraphCanvas.tsx`).
- An anonymous clip (no id of its own, only in a hand-written file) moved along its lane gets its id
  from the server, so the selection and Notes lose it.
- Automation points and a double-click that makes an automation clip: U7.
- A dragged clip is heard only when dropped (Q10).
- The app shots `ui-u4-grid.png` and `ui-u5-select.png` (Xvfb `:98`) are for after the merge into
  `8b-ui`.
- Lane B's cargo target (`~/m2-runs/wt/ui-b/target`) is deleted. Its server stays in
  `~/m2-runs/s8ui/bin/ui-b-5a34189` for the merge's runs; delete it with `~/m2-runs/s8ui/bin` at the End.

## U6: clip commands (lane B)

**In short**
- Right-click a clip: a menu with what you can do to it, each with its key.
- Right-click an empty spot in a lane: Add clip, Paste here.

  | To | Press |
  |---|---|
  | Copy, cut, paste clips | ⌘C, ⌘X, ⌘V. Paste puts them at the playhead, on the selected track |
  | Make a copy right after | ⌘D. A song part that ends just before a bar line goes onto the next bar |
  | Cut a clip in two at the playhead | ⌘E |
  | Glue clips of one lane into one | ⌘J |
  | Put the notes on the grid | Q (the grid Notes uses, 1/16 at first) |
  | Move clips a little | ← →. With ⇧, a bar |
  | Pick the clips in some bars | drag on an empty spot: a box shows, on bar lines |

- After a box, ⌘D copies the whole bars of the box, empty bars too.
- Each one is a single undo step. No toast: you see the result.
- The same commands are in the Edit menu of the app: Split at Playhead, Join Clips, Quantize.
- Checks: the web build, `s8-clips` (now 90 checks), 20 suites it could move, the menu tests, and
  copy and paste in the Linux app.

### What changed

| File | What |
|---|---|
| `web/src/lib/clips.ts` | The gestures `split`, `join`, `duplicate` (with a marquee's bars and the grid), `paste`, `quantize` and `points`. The selection keeps a marquee's bars (`range`) and drops clips an edit took away (`prune`). The clipboard: the copied clips as the lane format writes them, in this window. A quantize whose words read the same before and after is still named in Undo |
| `web/src/components/SongTimeline.tsx` | The commands, and their `canvas` hooks (⌘D, ⌘E, ⌘J, Quantize, ⌘C ⌘X ⌘V). Q and ← → (⇧: a bar) are the timeline's keys. Nudges asked for while an edit is on its way add up and go out as one move. The marquee on empty lane space (a click there still moves the start point). The clip menu and the lane menu. In the app, `beforecopy`, `copy`, `beforecut`, `cut`, `beforepaste` and `paste` are taken while clips are picked (or copied) and no field has the keyboard (M7). A click on a clip keeps the keyboard in the timeline, though Notes opens on it; a double-click gives it to Notes. Edits at the playhead read it to the 960th of a beat |
| `web/src/components/ClipMenu.tsx` (new) | A menu at the pointer: rows with their keys; a row that can't run is greyed and its tooltip says why |
| `web/src/App.tsx` | ⌘D's order: the code editor, the roll's notes, the timeline's clips, then the track |
| `web/src/lib/shell.ts` | `canvas.duplicate` |
| `web/src/components/GraphCanvas.tsx` | ⌘L loops a marquee's bars or the picked clips, before the clip Notes shows |
| `web/src/components/Shortcuts.tsx` | Edit: cut, copy, paste, the marquee, duplicate, split, join, quantize, move clips |
| `apps/music2-desktop/src/menu.rs` | Edit, on both menu bars, after Duplicate: Split at Playhead ⌘E, Join Clips ⌘J, Quantize (Q is the page's key). The test that said no item is ⌘E now says Split is (W7); a new test puts the three after Duplicate |
| `e2e/s8-clips.mjs`, `e2e/run.mjs`, `e2e/README.md` | Part 3 (35 checks): 90 in all |

### Where things go

| Command | Acts on | Then picked |
|---|---|---|
| ⌘D | the picked clips; after a marquee, its bars | the copies (after a marquee, the next bars too, so ⌘D again goes on) |
| ⌘E | the picked clips under the playhead; with none picked, every clip under it on the selected track, in the lanes it shows | both halves |
| ⌘J | the picked notes clips of one lane | the joined clip |
| ⌘V | the selected track's lane of the clips' kind (their own lane on their own track), else the lanes they came from | the pasted clips |
| Paste here (lane menu) | that lane, where the right-click was, on the grid | the pasted clips |
| Q | the picked notes clips, onto Notes' grid | the same |

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `s8-clips` | 90 of 90 |
| `taste`, `s4-clips`, `notes`, `fixes-notes`, `s8-songbar`, `gestures`, `layout` | 39, 28, 52, 37, 68, 30, 39 |
| `views`, `pc-keys`, `s8-tracks`, `s6-write`, `s4-meter`, `attention`, `g11-play` | 30, 37, 75, 36, 8, 57, 24 |
| `live3a`, `s6-builders`, `s4-problems`, `s4-attn`, `newsong`, `code` | 49, 55, 18, 13, 71, 58 |
| Server | `~/m2-runs/s8ui/bin/ui-b-5a34189` (B6, B7, B8), serving a copy of this page through `M2_WEB` |
| `cargo test --locked -p music2-desktop` (in `wt/ui-b`, its own target) | 8 passed; clippy `-D warnings` clean; `Cargo.lock` unchanged |
| In the Linux app on Xvfb `:98` (lane B's build) | Kick clip clicked, Ctrl+C, the ruler clicked at bar 10, Ctrl+V: the file has a copy at beat 36 (M7). The playhead at bar 5, the clip clicked, Ctrl+E (the menu bar's): two clips, 0–16 and 16–32 |
| Logs | `~/m2-runs/logs/ui-b-u6-suites-*.log`, `ui-b-u6-menu-*.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/ui-b-u6s` |
| Pictures | `~/m2-runs/s8ui/shots/lane-b/ui-u6-menu.png` (the clip menu in the Linux app), `ui-u6-paste.png` (the pasted copy at bar 10); `s8-clips-menu.png` in the suite's folder |

Captions for the owner:
- `ui-u6-menu.png`: right-click on a clip shows what you can do to it, with the keys.
- `ui-u6-paste.png`: the Kick clip, copied and pasted at bar 10.
- This is the Linux version. On your Mac it uses the Mac's own font and ⌘ keys.

**Red first:**
- `s8-clips` on U5's page (the same server): parts 1 and 2 pass, then no clip menu opens on a
  right-click and it stops there (`~/m2-runs/s8ui/scratch/e2e/ui-b-u6-red/s8-clips-fix.log`).
- The menu tests on the old `menu.rs` with the new tests: the two new ones fail, no Split item and
  no rows after Duplicate (`~/m2-runs/logs/ui-b-menu-red-*.log`).

**Found while building:**
- A right-click (or click) on a clip, with Notes showing nothing, opened the roll, which took the
  keyboard: ⌘D, Q and the arrows then went to the roll. The click now opens the roll first and takes
  the keyboard back.
- The engine counts in samples, so a playhead put on beat 40 read 39.99999: a paste landed there.

### Choices

| Choice | Why |
|---|---|
| Q quantizes to Notes' grid, not the timeline's | Notes' grid is the one notes are placed on. The menu row says which: "Quantize to 1/16" |
| ⌘V onto the selected track | As Logic. Clips copied from several lanes go back to their own lanes |
| No toast on copy | Paste in the menus turns on; a toast for every copy is noise |
| The clip menu has no icons | The ruler's menu has none; Copy and Duplicate would want the same icon |
| The marquee is always on bar lines | design-8b §7.4; ⌘D then keeps the song's bars |

### Left

- The marquee doesn't scroll the timeline when it reaches an edge.
- "Clear lane…" (design-8b §7.5) isn't in the lane menu: the plan's rows are Add clip, Paste, Hide.
- Copied clips stay in this window only: not in the system's clipboard, not across windows.
- Hide automation folds the track's automation, not one lane (there is no per-lane fold).
- A dragged or nudged clip is heard only when it lands (Q10).
- Lane B's cargo target is deleted at the end of the lane.

## U7: automation in place (lane B)

**In short**
- You can draw automation right on the timeline now.

  | To | Do |
  |---|---|
  | Make an automation clip | double-click an empty spot in an automation lane |
  | Add a point | click on the line |
  | Move a point | drag it. A small label says its value and where it is |
  | Delete a point | click it, then ⌫ |
  | Draw freely | press B, then drag across the clip. B again to stop |
  | Pick or move the whole clip | click or drag its thin strip above or below the line |
  | Take the automation out | right-click the lane's name: Remove automation |

- Remove automation writes the value at the playhead back into the code, and the lane goes. A toast
  says so, with Undo.
- In the Device view, a control that follows a lane has a small blue dot. Right-click it: Show lane,
  or Remove automation.
- A new lane is named after its control: "Decay", not "out-decay".
- Each edit is one undo step.
- Checks: the web build, the new `s8-automation` suite (43 checks), and 25 suites it could move.

### What changed

| File | What |
|---|---|
| `web/src/lib/points.ts` (new) | What a press on a curve hits (a point within 6 px, else the line within 6 px, else the clip's body), and the points a click, a drag or a paint make. Values keep to what the unit allows (a time or a frequency never below 0; 0–1 and −1–1 amounts in range). The picked point, kept with where it was, so an undo that moves the points unpicks it; the points being edited, drawn until the timeline shows them |
| `web/src/components/SongTimeline.tsx` | On an automation clip: a press near a point picks it and a drag moves it; on the line a point is added (and can be dragged on); in Draw mode a drag paints a point on each grid line, filling in between. A point wins over the clip's edge; the line doesn't. ⌫ deletes the picked point; Esc unpicks it. A double-click on an automation lane's empty space makes a clip, holding the lane's value. The lane's header has a menu: Remove automation, Hide automation. Draw mode shows a crosshair over automation clips. The selected point is `fg-strong` in a 2 px white ring |
| `web/src/lib/clips.ts` | `removeAutomation` (`POST /api/graph/automate/remove`, one undo step), `playhead()` shared, `InputPort.lane` |
| `web/src/components/ClipMenu.tsx` | `unautomate`: Remove automation with its toast and Undo, for the timeline and the Device view |
| `web/src/components/NodeGraph.tsx` | An input a lane drives: the lane's dot, a tooltip, and a right-click menu: Show Decay lane, Remove automation (KN-2) |
| `web/src/components/Shortcuts.tsx` | "Draw mode: notes and automation B" |
| `e2e/s8-automation.mjs` (new), `e2e/run.mjs`, `e2e/README.md` | The suite (43 checks), its row after `s8-clips` |
| `e2e/notes.mjs` | A double-click on an automation lane makes a clip now (it made none); the clip is picked by its top strip, away from its curve. Same count |
| `e2e/s8-clips.mjs` | The automation clip dragged over another unit's lane is grabbed by its top strip. Same count |

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `s8-automation` (new) | 43 of 43 |
| `s8-clips`, `notes`, `taste`, `pc-keys`, `s4-clips` (on the final page) | 90, 52, 39, 37, 28 |
| `fixes-notes`, `s6-builders`, `s5-knobs`, `ports`, `graph`, `cables`, `undo`, `effects` | 37, 55, 101, 46, 19, 12, 10, 12 |
| `views`, `layout`, `gestures`, `s8-tracks`, `s6-write`, `s6-fixes`, `attention` | 30, 39, 30, 75, 36, 19, 57 |
| `live3a`, `g11-play`, `code`, `s8-songbar`, `fixes-graph` | 49, 24, 58, 68, 42 |
| In the Linux app on Xvfb `:98` | Bass unfolded, a click on the Cutoff line at bar 3 and a drag: the file has a point at beat 9, 2638 Hz |
| Server | `~/m2-runs/s8ui/bin/ui-b-5a34189` (B8: Automate's names, Remove automation, `InputPort.lane`) |
| Logs | `~/m2-runs/logs/ui-b-u7-suites-*.log`, `ui-b-u7-suites2-*.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/ui-b-u7s`, `ui-b-u7s2` |
| Pictures | `~/m2-runs/s8ui/shots/lane-b/ui-u7-automation.png`; `s8-automation.png` in the suite's folder |

Captions for the owner:
- `ui-u7-automation.png`: Bass unfolded; a point added on the Cutoff line and dragged up. The dark
  dot with a white ring is the picked point.
- This is the Linux version. On your Mac it uses the Mac's own font and ⌘ keys.

**Red first:** `s8-automation` on U6's page (the same server): Automate and the "Decay" name pass
(B8), then the Device view has no input that follows the lane, and it stops there
(`~/m2-runs/s8ui/scratch/e2e/ui-b-u7-red/s8-automation-fix.log`).

**Found while building:**
- A drawing range is what the lane's values span (the server's rule). Painting near the bottom of a
  new Decay lane wrote negative seconds. Values now keep to what the unit allows, and may go past
  the drawn range: the next drawing widens.
- The app (not the browser) serves the page copied into `target/debug/web` when it was built. Lane
  B's app checks now set `MUSIC2_WEB_DIR` to the lane's page. U6's menu picture was taken again
  from the final page. At the merge, rebuild the app or set `MUSIC2_WEB_DIR` before the shots.
- U6 kept text from being selected during a marquee with `user-select: none` on the timeline. In
  WebKit that can stop typing in a field inside it (the track rename field). The marquee now clears
  any text selection as it goes instead. `s8-clips`, `s8-tracks`, `taste` and `s8-automation` ran
  again on this page: 90, 75, 39, 43 (`~/m2-runs/logs/ui-b-u7-suites3-*.log`).

### Choices

| Choice | Why |
|---|---|
| A point wins over the clip's edge; the line doesn't | A clip's end points sit on its edges. Grabbing the edge above or below the line still resizes |
| A click on a point or the line unpicks the clips | Then ⌫ deletes the point, never the clip behind it |
| Draw mode is the roll's B | One mode for both. Over automation clips the pointer is a crosshair |
| Remove automation writes the value at the playhead | design-8b HOME-2; at the start of the song it is the fallback, as written |

### Left

- Automation lanes are 32 px, so a clip's drawing is 14 px tall: away from the line, only thin
  strips pick or move the clip. Taller lanes (U13) would help.
- The drawing range follows the values (the server's rule), so the curve redraws after an edit that
  widens it. A frequency is drawn on a straight scale, not a log one.
- No curve bends (⌥-drag): that needs a field in the lane format (FMT-2).
- The lane's own node in the Device view has no menu; the input it drives does.
- Hide automation folds the track's automation lanes, not one lane.

