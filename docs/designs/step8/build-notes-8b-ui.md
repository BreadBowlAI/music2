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
| U1 (lane A) | M and S on every track. A pill at the top says "2 muted · 1 solo"; click it for the list and Clear all | done |
| U2 (lane A) | "Mute in song" in a track's menu: saved in its code, so the export leaves it out too. One undo step | done |

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

## U1: M, S and the mutes pill (lane A)

**In short**
- Every track header has **M** (mute) and **S** (solo). Click to turn one on or off.
- **⌥-click S** (Alt-click on a PC) plays only that track.
- **Drag** across several M (or S) buttons to set them all the same way.
- The **M** and **S** keys work on the selected track. ⌃M and ⌃S do too, on a Mac.
- While anything is muted or soloed, a blue **pill** at the top says so: "2 muted · 1 solo".
  Click it to see each track, with its own × and **Clear all**.
- These mutes are only for listening. They don't change the song's code or the export, and they are
  no undo step. They are kept when the song closes, so the pill tells you when a song opens muted.
- Checks: the new `s8-mute` suite (68 checks), and 10 suites that cover the header and the top bar.

### What the user sees

| Thing | Looks like |
|---|---|
| M, off | a 20 px square, 1 px `line-strong` border, "M" in `fg-muted` |
| M, on | `gray-700` fill, white "M" |
| S, on | `accent` fill, white "S" |
| Hover | a step darker (`press` when off), as the header is `hover` already |
| A muted track | name `fg-muted`, clips at 40% and grey (U0's look) |
| A track a solo leaves out | name `fg-faint`, clips at 40% and grey |
| Mutes pill | `accent-soft` fill, `accent-strong` words, just right of the status pill |
| Its list (360 px) | each track: colour, name, "Muted" or "Soloed", its ×. Kick's row adds "Still drives Duck". A foot line, "Only while you listen: the export has every track.", and Clear all |
| The pill's tooltip | "Mutes and solos while you listen. The export has every track." In a read-only window it starts "Not saved: this song is open in another window." |
| M's tooltip on muted Kick | "Muted · still drives Duck. Unmute (M)" |
| S's tooltip | "Solo (S) · ⌥-click: solo only this track" ("Alt-click" on a PC) |

### What changed

| File | What |
|---|---|
| `web/src/lib/overlays.ts` (new) | The overlay route's calls. The screen changes at once; requests go one at a time, in order; only the newest answer is shown (M6); a refusal puts the screen back and says the server's words in a toast. The pill's words, "Still drives …", and the track names |
| `web/src/components/MuteSolo.tsx` (U0's mount) | M and S: click, ⌥-click, drag across, keyboard Space or Return on a focused button. They never take focus (M2). Off only while the connection is gone. Fills `canvas.mute` and `canvas.solo` |
| `web/src/components/OverlayPill.tsx` (U0's mount) | The pill and its list |
| `web/src/components/Status.tsx` | While the mutes pill shows, the status pill's words are cut at 100 px (Q23). Its tooltip keeps all of them |
| `web/src/components/Shortcuts.tsx` | Tracks: "Mute / solo the selected track: M / S" and "Solo only this track: ⌥-click S" |
| `e2e/s8-mute.mjs` (new), `e2e/run.mjs`, `e2e/README.md` | The suite and its rows, after `s8-tracks` |

The pill counts only tracks of the song now (Q2). A track that is gone but still in
`overlays.json` is listed greyed, "Not in the song now", with its ×.

### The top bar at small widths (Q23)

A 77-letter song name, "Loading Surge XT, Airwindows Consolidated…" and "2 muted · 1 solo",
measured by `s8-mute`:

| Width | Name ends | Status pill | Mutes pill | Undo starts |
|---|---|---|---|---|
| 1100 | 620 | 661–797 | 805–919 | 944 |
| 1180 | 700 | 741–877 | 885–999 | 1024 |
| 1280 | 800 (732 in the Mac app) | 841–977 | 985–1099 | 1124 |

So 100 px for the status words leaves at least 41 px between the name and the status pill.

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `s8-mute` (new) | 68 of 68 |
| `taste` | 39 of 39 |
| `fx-fixes` (the top bar at 1100 to 1280 px) | 75 of 75 |
| `s8-tracks`, `layout`, `views`, `pc-keys` | 75, 39, 30, 37: all pass |
| `notes`, `s4-clips`, `gestures`, `fixes-notes` | 52, 28, 30, 37: all pass |
| Log | `~/m2-runs/logs/ui-a-u1-1017-062905-464483.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/lane-a/u1` |

All against the U0 server (`~/m2-runs/s8ui/bin/fba5147`) serving this lane's page through `M2_WEB`.

**Red first:** `s8-mute` on U0's page fails at its first check (no M or S in the header) and stops
at the first click on M (`~/m2-runs/s8ui/scratch/e2e/lane-a/u1-red`).

### Left

- Mute in the song, from the header menu, comes in U2.
- U12 (later): M and S on Mix strips.

## U2: mute in the song (lane A)

**In short**
- A track's menu (⋯, or right-click its header) has **Mute in song**. It writes `muted: true` in
  the track's code. So the song, and the export, leave the track out.
- It is one undo step. A toast says "Kick muted in the song", with Undo.
- A track muted in the song looks it: its name is **struck through**, its clips are **hatched**,
  and its M is lit.
- A click on that M opens the menu, with **Unmute in song** first. The M key and a drag across M
  buttons leave it alone, because unmuting it is an edit to the code.
- Checks: `s8-mute` grew to 99 checks; `taste`, `s8-tracks` and `pc-keys` pass.

### What changed

| File | What |
|---|---|
| `web/src/lib/overlays.ts` | `muteInSongCall`: `POST /api/graph/track/mute`, recorded as the undo step "mute in song" (or "unmute in song"), so Undo says "Undo mute in song (⌘Z)" and then "Undid mute in song" |
| `web/src/components/Tracks.tsx` | `muteInSong(module, muted)`: refused in words while the song can't be edited; the toast "Kick muted in the song" (or "unmuted") with Undo. The menu row "Mute in song", with the line "Saved in its code: the export leaves it out too", below Colour and automation. On a track muted in the song, "Unmute in song" is the first row. Off (with the reason as its tooltip) in a read-only window, while the code doesn't compile, or offline |
| `web/src/components/MuteSolo.tsx` | Fills `canvas.muteInSong`: Track ▸ Mute in Song on the selected track, either way |
| `e2e/s8-mute.mjs`, `e2e/run.mjs`, `e2e/README.md` | Part 2 (31 checks), and the read-only part's two more |

What U1 already did for this state: M lit `gray-700` (M5); a click on it opens the menu; the M key
says "Kick is muted in the song. Unmute in song is in its menu." and changes nothing; a drag across
M skips it; S lights but the state stays "muted-in-song", and S's tooltip says "Muted in the song:
unmute it to hear it".

### Checks

| Check | Result |
|---|---|
| `cd web && npm run build` (type-checks) | ok |
| `s8-mute` | 99 of 99 |
| `taste`, `s8-tracks`, `pc-keys` (the suites that open the header menu) | 39, 75, 37: all pass |
| Log | `~/m2-runs/logs/ui-a-u2-1017-063221-505368.log`; suite logs in `~/m2-runs/s8ui/scratch/e2e/lane-a/u2` |

**Red first:** the suite on U1's page stops where it clicks the menu's Mute in song: there is no
such row (`~/m2-runs/s8ui/scratch/e2e/lane-a/u2-red`).

### Left

- Nothing for U2. The export's question comes in U3.

