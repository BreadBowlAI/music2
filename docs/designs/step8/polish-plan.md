# Polish: the plan

For the `polish` track: branch `polish`, worktree `$SCRATCH/wt/pl`, made from 8b-ui's tip `29430e8` (main,
taste-next and all of 8b's screens). Written 2026-10-17 by a planning agent. Read only: files and `git`. Nothing was
built or run. File:line cites are at `29430e8`. Sizes: **S** under 2 hours of agent time, **M** half a day, **L** most
of a day *(estimates)*.

**The Decisions section overrides the rest of this file. So does an Attack section, if one is added.**

---

## In short

- This track makes the app nicer to use when you make a pop song. Four parts are built side by side.
- **First, the lead lays the ground (P0):** empty places for each part to fill, the new keys and menu rows, and one
  place where view settings are kept.

| Part | What you get | Size |
|---|---|---|
| **A. Mix** | M and S buttons on the Mix strips. A level meter on every strip. Change a track's instrument. Drum names (Kick, Snare) in Notes | M + M |
| **B. Timeline** | Zoom in and out. Taller tracks. The view follows the playhead (you can turn it off). Repeat a clip a few times. Markers on the ruler (Intro, Chorus) | M + M + M |
| **C. Songs** | A list of recent songs. The last song opens when the app starts. Save As. Rename a song and its folder. Change a track's ID. The delete question fixed. Your view settings are kept | M + M + M |
| **D. Notes** | A right-click menu on notes. Note tools: transpose, quantize, legato, humanize, swing. The song's key in the top bar. Automation drawn on the knob's own scale. Hide one automation lane | M + M + S |

- **Left for later:** meters after the fader, curves between automation points, sections, a picture of each song
  in the recent list, drum rows folded to the drums used.
- **About 2 days.** Day 1 morning: P0. Then the four parts. Day 2 afternoon: join them, review, the full check,
  pictures for you.
- **Nothing you hear changes.** Every song still exports to the same bytes.

---

## Decisions

The lead takes each decision as written. Nobody waits for an answer.

| # | Question | Decision |
|---|---|---|
| Q1 | Mix strip meters: before or after the fader? | **Before**, the same tap as the track headers (`core/runtime/src/taps.rs:1-9`: a track is tapped where its audio enters another module). A fader move doesn't move its strip meter; the meter's tooltip says "Kick's level, before its fader". Master's meter is the song's output, so it is after everything. After-fader taps need `core/runtime` (`lib.rs:1871`, 8a's area): left |
| Q2 | Strip meter: one bar or two? | Two bars, left and right, 3 px each with 1 px between (taste.md §2.8 "2×3 px"); Master 2×5 px. The header keeps its one bar (8b-ui Q22) |
| Q3 | Clip dots? | On track strips: a 4 px `danger` dot lights when a peak reaches 0 dBFS and stays until clicked (taste.md §2.8). Master has none: it ends in a −1 dB limiter (8b-ui W5) |
| Q4 | M and S on a strip shared by tracks ("Kick + Snare")? | M and S act on every track in it. When they differ: `aria-pressed="mixed"`, drawn as a dash (design-8b §2.3 step 4). ⌥-click S solos exactly those tracks |
| Q5 | Change instrument: what happens to the old instrument's settings? | Dropped: the new instrument starts at its own settings, as + Track does. If a setting holds anything but a plain number (a lane, an LFO), the change is refused: "Bass's cutoff follows other code. Change its instrument in code." First-song's bass is refused; the starter's bass is changed |
| Q6 | Where is Change instrument? | Track ▸ Change Instrument… (menu bar), the track header menu, and the Mix strip menu. It opens the + Track list, titled for the track, with its instrument ticked |
| Q7 | Drum names: which rows? | Rows with a name show it in place of the C labels; the key column widens from 56 to 88 px for that lane. Note labels say the name too. Folding to the drums used (taste NT-5) is left |
| Q8 | Zoom range and steps | 2 to 192 px per beat, ×1.5 a step, 24 at first (taste.md §3.2). Kept per song |
| Q9 | Zoom around what? | ⌘= and ⌘−: the playhead when it is on screen, else the middle of the view. Pinch and ⌘-scroll: the pointer. Z: the selected clips, or the whole song; Z again goes back |
| Q10 | Track heights | Drag a header's bottom edge: 28 to 160 px, in 4 px steps. ⌘↓ taller, ⌘↑ shorter (Logic's way round): 28, 44, 72, 120, on the selected track, or on every track when none is selected. A double-click on the edge puts back 44. Automation lanes get the same edge (24 to 160 px, 32 at first): they are thin to draw on (gaps #17). Kept per song |
| Q11 | Follow the playhead | On at first, kept per song (taste.md §5.1). While playing, the view pages when the playhead passes 85% of its width. Scrolling by hand pauses it until play starts again or the playhead is back in view |
| Q12 | Repeat a clip | Clip menu ▸ Repeat…: "Repeat [3] more times". The copies follow one after another. One undo step. 1 to 64 |
| Q13 | Markers | In `layout.json` `markers` (design-8b §2.1, §7.6), one per beat, names up to 40 characters. Added at a bar line from the ruler's menu, or at the playhead's bar from Transport ▸ Add Marker. A new one is "Marker 1" (2, 3…) and opens as a field, with a short list to pick: Intro, Verse, Pre-chorus, Chorus, Bridge, Drop, Outro |
| Q14 | Moving a marker | A drag snaps to the timeline's grid; holding ⌘ places it freely |
| Q15 | Hiding one automation lane: how does it come back? | A line under the track's lanes: "1 hidden lane · Show". Automate on that control shows it too. Kept per song |
| Q16 | Where view settings live (8b-ui Q7) | **All in `.music2/view.json`, per song**: zoom, follow, track and lane heights, hidden lanes, each view's grid and snap, the audition toggle. It is local and gitignored (`docs/architecture.md:458`, FMT-20), not the song's own files, and already keeps the loop (`crates/server/src/graph/view_file.rs`). A new song starts with the defaults. The dock and the browser stay per window, in the browser's storage, as today (the suites set them there: `e2e/fixlib.mjs:45`) |
| Q17 | Recent list | Kept in `~/.music2/recent.json`, newest first, 20 songs. The menu shows 10. A folder that is gone shows "Moved or deleted" with × to forget it |
| Q18 | Reopen at launch | Always the newest song in the list, when the app starts with no folder given. If the app closed while opening it last time, it doesn't try again: the start screen says why |
| Q19 | Save As | File ▸ Save As… (⇧⌘S). It saves a copy under a new name and opens the copy. The old song stays as it was. Without `.music2/` (mutes, view, undo journal) |
| Q20 | Rename | File ▸ Rename Song… opens a dialog: the name, and "Also rename its folder" (on). Renaming the folder closes the song and opens it again, so Undo starts over (the dialog says so). The top bar's double-click stays a quick rename of the name only |
| Q21 | Change track ID | As design-8b §11 says, without Restore and without the engine pass that keeps the track's instruments (a renamed track restarts its sounds). Only in the Code view's file list: right-click a track's file |
| Q22 | Key and scale: where it lives | `layout.json` `key: {"root": "A", "scale": "minor"}`: a song file, shared in git, and nothing that plays depends on it. Major and minor only; or none. More scales come with scale help (#16) |
| Q23 | The key field and the top bar's room | The field sits after the time signature. The song's name keeps clear of the measured left and right groups, not a fixed 480 px (8b-ui W6). `fx-fixes` and `s8-songbar` must pass |
| Q24 | Showing the key in Notes | While a key is set, rows out of the key are shaded instead of the black keys' rows. Nothing is locked |
| Q25 | Note tools act on what? | The selected notes; with none selected, every note of the clip. Each is one undo step |
| Q26 | Note tool amounts | Humanize: starts move up to ±10 ms, velocities up to ±8%. Swing: Light 55%, Medium 60%, Heavy 66%, on eighths when the roll's grid is 1/8 or wider, else sixteenths. Legato: each note ends where the next note starts (a chord's notes together). Quantize (Q): starts to the roll's grid |
| Q27 | Automation's scale | The control's own range (what its knob declares), widened to hold every point; on a log scale when its knob is (frequencies). Lines between points are drawn as they play: straight in value, so curved on a log scale |
| Q28 | Curves between points | **Cut.** A new field in the lane format (`core/song`) and new interpolation in `core/runtime` (8a's area): not "simply" |
| Q29 | `m2` commands for the new edits | Left (gaps #37). Agents can send the same edits as JSON to the routes, or a Tx |
| Q30 | The delete question's default button | Stays Delete. Only the keys change: Return and Space press the focused button, Tab goes round, no key reaches the page |
| Q31 | Where the zoom and follow controls go | A small group at the right end of the ruler, on `chrome`: Follow, Zoom out, Zoom in. Icons: `ZoomOut`, `ZoomIn` (taste.md §2.12), and `ArrowRightFromLine` for follow (added to taste.md's list at the End) |
| Q32 | Spelling of keys | C, C♯, D, E♭, E, F, F♯, G, A♭, A, B♭, B |
| Q33 | The roll's Z, ⌘↑ and ⌘↓ | Not the roll's: Z zooms the timeline; ⌘↑ ⌘↓ change track heights. The roll keeps ⌘-scroll for its own zoom |
| Q34 | Lanes need a change in another lane's file | They don't, after P0. If one turns out to, it asks the lead, who makes it in `polish` and the lanes merge it. Never edit another lane's file |

---

## Attack

Done: the findings (X1 to X45) and the decision for each are at the end of this file, in "## Attack" and its
"## Decisions". They override the rest of this file, Q1 to Q34 included.

---

## 0. Where things stand

| Item | What exists | What's missing |
|---|---|---|
| Mix M and S | Strips with pan and fader (`web/src/components/Mix.tsx`); an empty 24 px row for M and S (`Mix.tsx:201-202`). M and S on headers (`MuteSolo.tsx`, one module) | The buttons on strips, a shared strip's mixed state, Mute in song in the strip menu |
| Strip meters | Header and Song-row meters (`Meter.tsx`, `lib/meters.ts`, one bar of the louder side, `meters.ts` `onMeters`). The server sends `[l, r]` per track and `out` (`crates/server/src/graph/meters.rs`). The fader leaves room for a meter (`Mix.tsx:40-41`) | Meters on strips and Master, clip dots |
| Change instrument | Add track writes `track(…, ({ lane }) => pluck(lane.notes("notes")))` (`crates/editor/src/tracks.rs:252-276`); the list `tracks::instruments` | A builder that swaps the call |
| Drum names | `drums.kit` plays by GM note (`std/src/drums.ts:1-15`, `:50-77`). Descriptors have `editor: {category, description, insert}` (`core/script/src/component.rs:121-129`, `core/script/sdk/core.ts:769-774`, `:1116-1118`) | `editor.notes`; the names in the timeline's lane; the roll showing them (`NoteView.tsx:77` `pitchName`, `:905-983` `Keys`) |
| Zoom | `PPB = 24` (`SongTimeline.tsx:66`), used in about 40 places. The roll has its own zoom (`NoteView.tsx:224`) | All of it |
| Track heights | `ROW_H 44`, `LANE_H 32` (`web/src/lib/layout.ts`), `rowHeight` (`SongTimeline.tsx:76`) | All of it |
| Follow | A jump while playing, always on (`SongTimeline.tsx:345-350`) | Off switch, paging, pause on scroll, saved |
| Repeat | `duplicate` gesture (`crates/editor/src/clips.rs:85-91`, `:219`) | A count |
| Markers | `layout.json` keeps `markers` as read, nothing reads them (`crates/editor/src/layout.rs:179-180`, `:221`, `:246`) | Reading, writing, the ruler |
| Recent | Start screen with New Song, Open, Examples (`StartScreen.tsx`); the desktop opens a folder given at launch (`apps/music2-desktop/src/main.rs:41`) | The list, the menu, reopen at launch |
| Save As, Rename | `project::create` copies a folder without hidden files (`crates/server/src/project.rs:301`, `:429`) and names the song (`:460`). Title edit (`crates/editor/src/song.rs:300`) | Routes, dialogs, folder rename |
| Track ID | `Rekeyed` is computed by the delta (`core/session/src/tx/delta.rs:593`); overlays follow it (`crates/server/src/graph/actor.rs:808`); `Rename` op exists (`core/song/src/tx.rs:89-93`) | The builder (design-8b §11.1), the route, the dialog |
| Delete question | `Tracks.tsx:501-556`: Return always runs Delete; Space reaches the page | The fix of `af99eec` (`ExportAsk.tsx:47-66`) |
| View settings | `view.json` keeps the loop (`view_file.rs`, `graph.rs:784`, `:973`). Grids and the audition toggle in `localStorage` (`grid.ts:125-159`, `audition.ts:25`, `:45`, `:154`): empty each launch, since the page's port changes | A route to write `view.json`, and the page reading it |
| Note menu, tools | The roll: select, move, nudge, transpose with arrows, duplicate (`NoteView.tsx:323-391`). No right-click menu | The menu and four tools |
| Key and scale | taste.md §3.3 keeps a place at x = 472 | Its home, the field, the roll |
| Automation scale | Linear over the lane's own values (`SongTimeline.tsx:561-565`, `:649`, `:1779-1781`), drawn by `Curve` (`:1997`). The view knows each knob's range and `log` (`crates/editor/src/view.rs:177-200`) and the lane that drives an input (`view.rs:899-940`) | Using them |
| Hide one lane | A folds all of a track's lanes (`shell.ts` `toggleLanes`) | One lane |

---

## 1. How the track runs

### 1.1 Branches and worktrees

- `polish` in `$SCRATCH/wt/pl` is the lead's. P0 and the End are built there.
- After P0a lands (§2), the lead makes four lanes, each from `polish`:
  ```
  cd $SCRATCH/wt/pl && git config rerere.enabled true
  for l in a b c d; do git worktree add $SCRATCH/wt/pl-$l -b polish-$l polish; done
  ```
- Lanes merge `polish` into their branch when P0b lands, and whenever `polish` moves (main merged in). Never rebase.
- Each lane merges into `polish` at its end, by the lead (§8). Never push.
- Never touch `wt/tn`, `wt/ui`, `wt/8a`, or tmux sessions `m2`, `1`, `2`, `ssh_tmux`.

### 1.2 Builds

Every cargo, npm or music2 run has `CARGO_HOME=/home/<user>/.cargo RUSTUP_HOME=/home/<user>/.rustup`. Builds are
`--locked`. Never `cargo update`; if cargo wants to change `Cargo.lock`, stop and look.

| Who | Target | Builds |
|---|---|---|
| Lead (`wt/pl`) | `$SCRATCH/s8c/target-pl` | P0's Rust tests, the desktop app for pictures, the End's full check |
| Lane X (`wt/pl-x`) | `$SCRATCH/s8c/target-x` | `cargo build --locked -p m2-server --example headless -p m2`, and the tests and clippy of the crates it touches |
| Lane C, also | `target-c` | `cargo test --locked -p music2-desktop` and its clippy: C2 changes `menu.rs`, `main.rs` and `actions.rs`. The one exception to "headless and m2 only" |

- After each build, copy the binaries per commit, so a rebuild never swaps one under a running suite:
  `B=$SCRATCH/s8c/bin/<lane>-<sha>; mkdir -p $B/examples; cp $T/debug/examples/headless $B/examples/; cp $T/debug/m2 $B/`
  (`run.mjs` finds `m2` one folder above the server's, `e2e/run.mjs:125`).
- Web: `cd web && npm ci && npm run build` (it type-checks, `web/package.json`). Once per worktree also `cd e2e && npm ci`.
- Long jobs go through `TW_WAIT=<seconds> $SCRATCH/tw <name> '<cmd>'` (cargo builds and tests, suite runs over a few
  minutes, the full check). Logs in `$SCRATCH/logs/`.
- At its end each lane deletes `target-<lane>` and `bin/<lane>-*`. The lead deletes `target-pl` and `bin/` after the
  End's pictures. (MEMORY: scratch targets once filled 390 GB.)

### 1.3 Suites

- **Tools.** P0 copies `$SCRATCH/s8ui/tools/*` to `$SCRATCH/s8c/tools/` and changes only the copies:
  - `lane-suites.sh <lane> <out> <suite>…`: a copy of `ui-suites.sh` with `repo=$SCRATCH/wt/pl` for
    `pl`, else `$SCRATCH/wt/pl-<lane>`. It copies the page and the suites into `<out>` first. `DIST=<folder>`
    serves an older page (for red first). `M2_HEADLESS` is required.
  - `app-shot.sh`: display `:96` by default, scratch under `$SCRATCH/s8c/scratch`, and `KEEP_HOME=<dir>`
    to reuse a HOME across two launches (C2's reopen check).
  - `xkey`, `xkey.c`: as they are.
- **Running:**
  ```
  M2_HEADLESS=$SCRATCH/s8c/bin/a-<sha>/examples/headless \
    $SCRATCH/s8c/tools/lane-suites.sh a $SCRATCH/s8c/scratch/e2e/a1 pl-mix taste s8-mix
  ```
  `M2_WEB` is set by the script to the copied page. `--repo` is the lane's worktree, which built the server, so std,
  the templates and the examples are the lane's. Each suite gets a scratch HOME under `<out>` and `M2_AUDIO=none`
  (`e2e/run.mjs:213-214`): never the real `~/.music2` or `~/Music`.
- New suites run as a Mac page and open one PC page each (`open(…, { pc: true })`, `e2e/fixlib.mjs:32`) for the Linux
  words ("Ctrl+=", "Alt+←").
- New lint states: P0 moves `lint()` out of `e2e/taste.mjs:19-52` into `e2e/lintlib.mjs`. Each new suite lints its
  new states itself (a menu open, a dialog, the recent list), so no lane edits `taste.mjs`.
- Rust tests never touch the real `~/.music2`: anything that keeps a file there takes its folder as an argument
  (C2's `recent.rs`), and tests pass a temp folder.

### 1.4 Checks per slice

1. The Rust tests and `cargo clippy --locked … --all-targets -- -D warnings` of the crates the slice touches.
2. The web build.
3. `taste`.
4. The slice's own suite.
5. The suites it moves (listed per item).

The full `ci/check.sh` runs once, at the End.

### 1.5 Red first

Every new suite check and Rust test that tests behaviour is shown failing first:
- a suite: run it on the page and server from before the change (`DIST=` the old `web/dist` copy, `M2_HEADLESS=` the
  old `bin/` copy), and copy two or three `FAIL` lines into the build notes;
- a Rust test: run it with the change taken out (or before it is made), and copy the failing assert.

### 1.6 Commits and build notes

- Small and green. Subject in plain words, a blank line, a short body, then exactly:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01R1SbPDdWrZoQ7z8dxU28hJ
  ```
- Each slice adds its section to `docs/designs/step8/build-notes-polish.md` in the same commit: what changed, tests,
  red first, what's left, and two lines for the End: **For gaps.md** (what to say is done, what is left) and
  **Contract** (any `docs/architecture.md` line that should change).
- P0 makes that file with one heading per part, in order: `## P0`, `## Lane A`, `## Lane B`, `## Lane C`,
  `## Lane D`, `## Merge`, `## End`, each with a line under it. A lane writes only under its own heading, so the
  merges don't meet.

### 1.7 Pictures

Only the lead, at the End, from the desktop app on the invisible Xvfb screen `:96`, never a real screen, with a
scratch HOME under `$SCRATCH/s8c/scratch/` and `M2_AUDIO=none`. States are written into a copy of the song
first (`view.json` for zoom and heights, `layout.json` for markers and the key, `recent.json` for the start screen),
not clicked. Each picture gets one plain line, and: "This is the Linux version. On your Mac it uses the Mac's own font
and ⌘ keys."

### 1.8 Writing for the owner

The In short sections, the build notes' In short, captions, HANDOFF and every word in the UI: short sentences, plain
words, tables. The track's ID is "ID", never "key"; "key" in the UI means only the musical key (taste.md:71, :120).

---

## 2. P0: the seams (lead) — M

P0 adds no behaviour a user can reach, except where it says so. It exists so no two lanes edit the same file.
It lands in two commits. Lanes start after **P0a**; they merge **P0b** when it lands and don't touch the files it
changes before that.

### P0a — the shared rows, stubs and types

**Rust**
- `crates/editor/src/lib.rs`: `pub mod swap; pub mod note_names; pub mod markers; pub mod rekey; pub mod songkey;`,
  each a new file with its doc comment and nothing else (plus `markers::read` and `songkey::read`, below).
- `crates/editor/src/tracks.rs`: `taken`, `channel_keys`, `track_file`, `the_mixer`, `quoted` become `pub(crate)`.
  After this, no lane edits `tracks.rs`.
- `crates/editor/src/layout.rs`: `Layout` gains `key: Option<J>`, read (`"key"`) and written after `markers` by
  `canonical`, kept as read like `markers`.
- `crates/editor/src/markers.rs`: `pub struct Marker { beat: f64, name: String }` and `pub fn read(&Layout) -> Vec<Marker>`
  (valid entries, sorted). `crates/editor/src/songkey.rs`: `pub struct SongKey { root: String, scale: String }` and
  `pub fn read(&Layout) -> Option<SongKey>`.
- `crates/editor/src/timeline.rs`: `TimelineView` gains `markers: Vec<Marker>` and `key: Option<SongKey>` (in
  `timeline()` and `last_known()`); `TimelineLane::Notes` gains `names: BTreeMap<u8, String>`, filled by
  `note_names::of(rev, module, lane)`, which returns an empty map until lane A fills it.
- `crates/server/src/graph.rs`: `mod instrument; mod markers; mod projects; mod songkey;` (files with an empty
  `impl Graph {}` each, as `meters.rs` does). After this, lanes add `Graph` methods only in their own file.
- `crates/server/src/routes.rs:92`: four more `.merge(…)` lines, for `routes_mix.rs` (A), `routes_view.rs` (P0b's view
  route, then B's markers), `routes_project.rs` (C) and `routes_notes.rs` (D): new files, each a router like
  `routes_8b.rs:30-35`, with the `writes` layer for its writes. `crates/server/src/lib.rs`: the four `mod` lines,
  and `pub mod recent;` (an empty file, lane C's).
- `apps/music2-desktop/src/menu.rs`: every static row of every lane, with its accelerator, in both menu bars:

  | Id | Title | Accelerator | Menu |
  |---|---|---|---|
  | `save_as` | Save As… | `CmdOrCtrl+Shift+S` | File, after Save |
  | `change_instrument` | Change Instrument… | — | Track, after Rename |
  | `zoom_in` | Zoom In | `CmdOrCtrl+=` | View, a new group first |
  | `zoom_out` | Zoom Out | `CmdOrCtrl+-` | View |
  | `zoom_fit` | Zoom to Fit | — (Z, the page's key) | View |
  | `taller` | Taller Tracks | `CmdOrCtrl+Down` | View |
  | `shorter` | Shorter Tracks | `CmdOrCtrl+Up` | View |
  | `follow` | Follow Playhead | `CmdOrCtrl+Shift+F` | Transport, after Loop Selection |
  | `add_marker` | Add Marker | — | Transport, a new group |
  | `prev_marker` | Previous Marker | — (⌥←, the page's) | Transport |
  | `next_marker` | Next Marker | — (⌥→, the page's) | Transport |

  New tests: each accelerator parses with muda's `Accelerator::from_str` (*not checked:* the names it takes for `=`
  and `-`; the test settles them), and the new rows' places. The existing tests (`menu.rs:244-341`) keep passing.
  After P0 only lane C edits `menu.rs` (Open Recent).

**Web**
- `web/src/lib/graph.ts`: export `request`; types `Marker`, `SongKey`, `TimelineView.markers`, `TimelineView.key`,
  the notes lane's `names?: Record<string, string>`, and `GraphState.local?: Record<string, unknown>` (P0b fills it).
  After this, lanes put their API calls in their own `lib/` file.
- `web/src/App.tsx`, the only edit any lane needs there, made once:
  - `chord()` (`App.tsx:50-81`): `=` or `+` → `zoom_in`, `-` → `zoom_out`, ⇧F → `follow`, ↑ / ↓ → `shorter` / `taller`,
    ⇧S → `save_as`.
  - `key()` (`App.tsx:34-46`): `z` → `zoom_fit`; ⌥← / ⌥→ → `prev_marker` / `next_marker` (with `preventDefault`, so
    WebKitGTK doesn't go back).
  - `appCommand`: each new id calls a `canvas` hook (`zoom(dir)`, `zoomFit()`, `follow()`, `height(dir)`,
    `marker(op)`, `changeInstrument(module)`), or for `save_as` and `rename_song` a function imported from C's stub
    files `SaveAsDialog.tsx` and `RenameSongDialog.tsx` (the stub of `openRenameSong` calls `canvas.renameSong?.()`,
    today's behaviour; the stub of `openSaveAs` does nothing).
  - ⌘↑ ⌘↓ in the code editor go to Monaco (`editorHooks.chord`, as ⌘D does; `CodeView.tsx` gets the two names); in a
    text field (the app's menu fires there too) they move the caret to the start or end, as `deleteBack` does for ⌘⌫.
- `web/src/lib/shell.ts`: the new `canvas` hooks, declared (`shell.ts:180-201`).
- `web/src/components/TopBar.tsx`: mounts `<KeyField graph={graph} status={status} />` after `MeterField`, from
  `KeyField.tsx` (D's), which renders nothing yet.
- `docs/designs/step8/build-notes-polish.md`: the headings (§1.6). `$SCRATCH/s8c/tools/` (§1.3).

**Checks:** the web build; `taste`, `pc-keys`, `start`, `notes`, `s8-tracks`, `s8-songbar`, `code` (counts
unchanged); `cargo test` and clippy of `m2-editor`, `m2-server`, `music2-desktop`. Copy the server to
`bin/pl-<sha>/`. Red first: none (no behaviour).

### P0b — the extractions and the view store

**The view store** (the one behaviour P0 adds; Q16):
- `POST /api/graph/view {session, set: {key: value | null}}` in `routes_view.rs`: merges into `.music2/view.json`
  through `view_file::write` (null deletes). `loop` is refused (it has its own route). Not a write in `WRITES`: in a
  read-only window it answers 200 and keeps the values in memory only (design-8b §2.2).
- `GraphState.local`: `view.json` as read when the song opened, without `loop`, with what this session set since.
- `web/src/lib/local.ts` (new): `useLocal(key, fallback)`, `setLocal(key, value)`. Seeded from `graph.local` when the
  session changes; writes post 300 ms after the last change.
- Keys, and which lane uses each: `zoom`, `follow`, `heights` (B); `hidden` (D); `grid`, `audition` (C).
- Rust test: `view_settings_merge_and_keep_the_loop` (a set, a null, a refused `loop`, a read-only window). Red
  first: the route doesn't exist (404).

**Extractions** (each changes nothing on screen; the suites that cover them stay green with the same counts):

| What | From | To | For |
|---|---|---|---|
| The piano keys | `NoteView.tsx:905-983` `Keys` | `web/src/components/RollKeys.tsx` | A (drum names) |
| A row's name, and the key column's width | `NoteView.tsx:77` `pitchName`, `:43` `KEYS_W`, the note label `:764`, the tooltips `:736`, `:818` | `web/src/lib/rowNames.ts`: `rowName(lane, pitch)`, `keysWidth(lane)` | A |
| A control lane's scale: value to height and back, and the drawn line | `SongTimeline.tsx:561-565`, `:649`, `:1779-1781`, `Curve` `:1997` | `web/src/lib/laneScale.ts`: `laneScale(module, lane, view)` → `{level, value, path}`, linear as today | D |
| Hiding one lane | — | `web/src/lib/hiddenLanes.ts` (`useHidden(module)`, `hide`, `show`; in memory), `HiddenLanes.tsx` (`<HiddenRow module>`, renders nothing); the filter in `rowsOf` (`SongTimeline.tsx:253`); a "Hide this lane" row in `TimelineMenu`'s header items (`:1702-1705`) | D |
| The delete question | `Tracks.tsx:482-556` (`joined`, `said`, `DeleteDialog`; `what` at `:132` is exported) | `web/src/components/DeleteDialog.tsx`, still mounted by `TracksUi` | C |
| The lint | `e2e/taste.mjs:19-52` | `e2e/lintlib.mjs` | all |

**Checks:** web build; `taste`, `notes`, `fixes-notes`, `s8-audition`, `s8-automation`, `s8-clips`, `s8-tracks`,
`s8-songbar` (counts unchanged); the Rust test above, red then green.

---

## 3. Lane A: Mix and sounds — `wt/pl-a`, branch `polish-a`

### A1. M and S on the Mix strips — S

**Sees and does**
- Each track strip shows M and S in its empty row (`Mix.tsx:201-202`), centred, 20 px each, 2 px apart: the header's
  look and tooltips (taste.md §2.9). A click, ⌥-click S (Alt-click on Linux), a drag across strips' M buttons: as in
  headers. M on a strip and M on the header are the same state.
- A strip of two tracks ("Kick + Snare"): M and S act on both; mixed shows a dash (Q4).
- No M and S on Master, nor on a channel no track reaches (MON-1).
- The strip menu (`Mix.tsx:400-457`, one-track strips) gains Mute in song (the header menu's row and words, one undo
  step with its toast) and Change instrument… (A3).
- "Still drives Duck" in M's tooltip on a muted kick strip, from `overlays.feeds`, as on the header.
- Focus: the buttons never take focus (`keepFocus`), so Space still plays (8b-ui M2).

**Back end:** none. `POST /api/graph/overlay` as headers use it (`web/src/lib/overlays.ts`).

**Tests** — suite **`pl-mix`** (kind `fix`, new, about 30 checks *estimate*), after `s8-mix` in `run.mjs`:
- Kick's strip has M and S (red first: `FAIL the Kick strip has M and S`); M mutes Kick (`overlays.mute` is
  `["kick"]`) and the header's M lights; ⌥-click S on Bass solos only Bass; a drag across three strips' M mutes three;
  Space after a click on M plays.
- A shared strip: the suite writes `song.ts` with `drums: sum(kick.out, hats.out)` first (a file write, then waits for
  the revision); its M mutes both; one unmuted → `aria-pressed="mixed"`.
- The strip menu's Mute in song writes `muted: true` and Undo takes it back.
- A PC page: S's tooltip says "Alt-click".
- Lint: the Mix view with mutes on.

**Moves:** `s8-mix`, `s8-mute`.

**Files:** `Mix.tsx`, `MuteSolo.tsx` (a `modules` list, the mixed state), `lib/overlays.ts`, `e2e/pl-mix.mjs`.

### A2. Meters on the strips — S

**Sees and does**
- Each track strip: two 3 px bars (left, right) beside the fader's track, the fader's travel tall, the same colours,
  hold and fall as the header meter (taste.md §2.8). Tooltip: "Kick's level, before its fader" (Q1).
- Master: two 5 px bars, the song's output. Tooltip: "The song's level".
- Clip dot: 4 px `danger` above a track strip's meter when a peak reaches 0 dBFS; it stays until clicked. Tooltip:
  "Went over 0 dB. Click to clear." None on Master (Q3).
- A strip with no track (a channel no track reaches) has no meter.

**Back end:** none. The `meters` message already has `[l, r]` per track and `out`.

**Tests** — `pl-mix`, about 12 more checks:
- While first-song plays: the Hats strip's two meters read above the floor (`aria-valuenow`); Master's too (red
  first: `FAIL the Hats strip has a meter`).
- After stop, every strip meter reads the floor within a second.
- A fader moved to −30 dB doesn't move its strip's meter (pre-fader), and the tooltip says "before its fader".
- Clip dot: the suite rewrites `tracks/hats.ts` to return `scale(hat(lane.notes("hits")), 8)` (`scale` from
  `std/math`), so Hats' peaks pass 1.0 where it is tapped; the dot lights; a click clears it; Master has no dot.
- Lint: the Mix view playing.

**Moves:** `s8-meters`, `s8-mix`.

**Files:** `Mix.tsx`, `Meter.tsx` (a `channels` prop, the dot), `lib/meters.ts` (keeps each channel, latches the clip),
`e2e/pl-mix.mjs`.

### A3. Change a track's instrument — M

**Sees and does**
- Track ▸ Change Instrument… (menu bar, on the selected track), Change instrument… in the header menu (after Rename),
  and in the strip menu. Disabled with the lock's reason while the code doesn't compile or the window is read only.
- It opens the + Track list (`Tracks.tsx:271-361`) at the menu, titled "Change Bass's instrument", its search field
  focused ("Find an instrument…"), the current instrument ticked and active. Typing filters, ↑↓ move, Return picks,
  Esc closes. The foot shows the code it writes: `pluck(lane.notes("notes"))`.
- Picking writes it, as one undo step, with a toast: "Bass plays Pluck now" [Undo]. The track keeps its name, colour,
  lane, notes, mixer channel, effects and mute.
- Refused, in a toast with [Show code]:
  - a setting that isn't a plain number: "Bass's cutoff follows other code. Change its instrument in code." (Q5);
  - no instrument from the list found: "Clap is built from parts, not one instrument. Change it in code.";
  - two: "Bass plays 2 instruments. Change them in code.";
  - a plugin: "Bass plays a plugin. Change it in code."

**Back end**
- `crates/editor/src/swap.rs`: `pub fn swap(s: &mut Session, module: &str, component: &str) -> Result<Edited, EditError>`.
  1. `ready(s)`; the component is in `tracks::instruments(s)`.
  2. In the track's module, the call sites whose component is an instrument (a category and notes first,
     `component.rs:174-180`). Exactly one, or refuse (words above).
  3. Its options (the second argument): each must be a number, or a unit call of one (`hz(420)`, `ms(40)`), else refuse.
  4. Ops (one `TextEdit` of the file the call is in):
     - the call's text becomes `<name>(<its first argument, as written>)`, `name` as `track_call_text` writes it
       (`synth.pad`, `pluck`);
     - an import of the new binding: none if there is one; else a specifier added to an import from the same module;
       else a new import line after the last import (`sites::import_place`);
     - the old binding's specifier is taken out when nothing else in the file uses it (`sites::imports_of`).
  5. `Expect::Confined`: that file, the call's range and each import's line; no lane; no module added or removed.
  6. The planner's assertion: only nodes of `module` change; no value of another module changes; the lane read is the
     same lane.
  7. Labels: `("Bass", "Bass plays Pluck")`.
- `crates/server/src/graph/instrument.rs`: `Graph::swap_instrument(session, revision, module, component)`.
- `crates/server/src/routes_mix.rs`: `POST /api/graph/track/instrument {session, revision, module, component}`, a write.
- `web/src/lib/instrument.ts`: the call, the toast, the refusal's [Show code].

**Tests**
- Rust `crates/editor/src/swap_tests.rs` (each red first: `swap` returns its stub error):
  `swap_replaces_the_call_and_its_import` (first-song's hats → `drums.kit`: `import { drums } from "std/drums"`,
  `hat` taken out); `swap_keeps_a_used_import`; `swap_refuses_a_setting_that_follows_code` (first-song's bass);
  `swap_refuses_two_instruments`; `swap_drops_plain_settings` (the starter's bass → pad); `undo_puts_every_byte_back`;
  `the_change_stays_in_the_track`.
- Suite **`pl-sounds`** (kind `fix`, new, about 30 checks *estimate*, with A4), after `pl-mix`:
  - the header menu has Change instrument… (red first: `FAIL the Hats menu has Change instrument…`); it opens the list
    with Hi-hat ticked; typing "kit" and Return writes `drums.kit(lane.notes("hits"))` in `tracks/hats.ts`; a toast
    says "Hats plays Drum kit now"; its Undo puts the file back byte for byte;
  - Bass is refused with the words; [Show code] opens `tracks/bass.ts`;
  - the strip menu's row does the same; Track ▸ Change Instrument… via `appCommand` on the selected track;
  - a PC page: the list's keys; lint: the list open in change mode.

**Moves:** `s8-tracks` (the + Track list), `s6-builders`.

**Files:** `crates/editor/src/swap.rs`, `swap_tests.rs`, `crates/server/src/graph/instrument.rs`, `routes_mix.rs`,
`web/src/lib/instrument.ts`, `Tracks.tsx` (`InstrumentPicker` gets a `swap` mode; `TrackMenu` a row; nothing else in
the file), `Mix.tsx` (strip menu row), `e2e/pl-sounds.mjs`.

### A4. Drum names in Notes — M

**Sees and does**
- Notes on a lane the drum kit plays: each named row's key says its drum ("Kick", "Snare", "Closed hat") in `text-xs`,
  where other lanes show C1, C2…; the key column is 88 px wide (Q7). A note's label and tooltip say the drum:
  "Snare · beat 2 · 1/16 · velocity 80%".
- Rows with no name keep the piano look and no label. Other instruments' lanes don't change.

**Back end**
- `core/script/sdk/core.ts`: `editor.notes?: Record<number, string>`: whole numbers 0 to 127, names one line of up to
  24 characters; checked where `editor` is (`:1116-1118`), emitted at `:1181`. `core/script/src/component.rs`:
  `Editor.notes: BTreeMap<u8, String>` (`#[serde(default)]`).
- `std/src/drums.ts`: `drums.kit`'s `editor.notes`, the GM notes the kernel plays, by the comment at `drums.ts:3-7`
  and the `std:drum` kernel's map (A checks each against the kernel): 36 Kick, 37 Rim, 38 Snare, 39 Clap, 41 Tom 1,
  42 Closed hat, 43 Tom 2, 44 Pedal hat, 45 Tom 3, 46 Open hat, 47 Tom 4, 48 Tom 5, 49 Crash, 50 Tom 6, 51 Ride,
  54 Tambourine, 56 Cowbell, 69 Shaker, 70 Shaker 2, 75 Claves. Kernels don't change, so `std.wasm` doesn't.
- `crates/editor/src/note_names.rs`: `of(rev, module, lane)`: the call the notes lane feeds in the accepted
  revision's IR (its frame's ComponentId, PKG-9), and that descriptor's `editor.notes`; empty when none.
- Web: `lib/rowNames.ts` (`rowName` from `lane.names`, else the pitch's name; `keysWidth` 88 or 56) and
  `RollKeys.tsx` (labels every named row).

**Tests**
- Rust: `core/script` `editor_notes_are_read_and_checked` (a pitch of 128 and a two-line name are refused, red first:
  `editor has no field "notes"`); `crates/editor` `a_drum_kit_lane_names_its_notes` (red first: the map is empty) and
  `a_pluck_lane_has_no_names`.
- `pl-sounds`: a Drum kit track added (`/api/graph/track/add`), its clip opened in Notes: key 36 says "Kick" (red
  first: `FAIL key 36 says Kick: C2`); key 38 "Snare"; a note on 42 is labelled "Closed hat"; the key column is
  88 px; Keys' roll still shows "C3"; lint.

**Moves:** `notes`, `s8-audition` (the keys play), `s6-builders`, `s8-tracks` (the instrument list reads
descriptors).

**Files:** `core/script/sdk/core.ts`, `core/script/src/component.rs`, `core/script/src/tests.rs`, `std/src/drums.ts`,
`crates/editor/src/note_names.rs` (and its tests), `web/src/lib/rowNames.ts`, `RollKeys.tsx`, `e2e/pl-sounds.mjs`.

---

## 4. Lane B: Timeline — `wt/pl-b`, branch `polish-b`

### B1. Zoom — M

**Sees and does**

| Action | Mac | Linux |
|---|---|---|
| Zoom in / out (View ▸ Zoom In, Zoom Out) | ⌘= / ⌘− | Ctrl+= / Ctrl+− |
| Zoom around the pointer | pinch, or ⌘-scroll | Ctrl-scroll (pinch *not checked*) |
| Zoom to the selected clips or the whole song; again: back (View ▸ Zoom to Fit) | Z | Z |
| The buttons | at the right end of the ruler: Follow, `ZoomOut`, `ZoomIn` (Q31) | same |

- 2 to 192 px a beat, ×1.5 a step, around the playhead or the pointer (Q8, Q9). Kept per song (`local.zoom`).
- The ruler numbers thin to every 2, 4 or 8 bars when bars are under 40 px (taste.md §3.2); tempo marks, the loop,
  clips, automation, the grid and the playhead all follow. The grid chip's Auto follows the zoom (`grid.ts` `stepOf`
  takes the px a beat already).
- Buttons: 24 px ghosts, `keepFocus`, tooltips "Zoom in (⌘=)", "Zoom out (⌘−)" (`chord()` spells Linux's).
- At the ends a button is disabled ("Zoomed in all the way").

**Back end:** none (P0b's view store).

**Web:** `web/src/lib/zoom.ts` (new): the store, the steps, the anchor maths, Z's "back". `SongTimeline.tsx`: `PPB`
(`:66`) becomes the store's value everywhere it is used; wheel and gesture handlers on the lanes' scroller (Chrome
and WebKitGTK send a ⌘/Ctrl wheel for a pinch; WKWebView on the Mac sends `gesturestart`/`gesturechange`, which must
`preventDefault` so the page itself doesn't zoom); `canvas.zoom`, `canvas.zoomFit`; the ruler's cluster.

**Tests** — suite **`pl-zoom`** (kind `fix`, new, about 45 checks *estimate*, with B2 and B3), after `s8-automation`:
- ⌘= makes the Bass clip 1.5 times as wide (red first: `FAIL ⌘= zooms in: clip width 96 → 96`), around the playhead
  (its x on screen stays within 1 px); ⌘− back; the buttons the same.
- ⌘-wheel at a clip's start keeps that beat under the pointer; a Ctrl-wheel (Chrome's pinch) the same.
- Z with Bass's clip selected: the clip fills about the view's width; Z again: back to the zoom and scroll before.
- At 4 px a beat the ruler shows every 4th bar number.
- After two ⌘= (54 px a beat) the grid chip's Auto is 1/16 (it is 1/8 at 24), and a dragged clip lands on sixteenths.
- Close and reopen the song (`/api/graph/close`, `/open`): the zoom is back; `view.json` has `zoom`.
- PC page: the tooltip says "Ctrl+=". Lint: zoomed in, zoomed out.

**Moves:** `s8-clips`, `s4-clips`, `s8-automation`, `s8-songbar` (the ruler), `layout`, `views`, `gestures`.

### B2. Taller tracks — M

**Sees and does**
- The bottom edge of each track header: a 6 px hit area, `row-resize`; drag it (28 to 160 px, 4 px steps). A
  double-click puts back 44. Tooltip: "Drag to change the height · double-click for the usual height".
- View ▸ Taller Tracks ⌘↓ / Shorter Tracks ⌘↑ (Ctrl+Down / Ctrl+Up): 28, 44, 72, 120, on the selected track, or every
  track when none is selected (Q10).
- The track's other notes rows take the same height. Automation lanes: the same edge on their headers, 24 to 160 px.
- M, S, the meter and the name stay where they are, from the top; the meter stays 28 px.
- Kept per song: `local.heights`, `{"bass": 72, "bass.cutoff": 64}`.

**Web:** `web/src/lib/heights.ts` (new: the store over `local`); `SongTimeline.tsx` (`rowHeight`, the rows, the
playhead's and marquee's sums, `songH`); `TrackHeader.tsx` (the edge); `lib/layout.ts` (the sizes as constants);
`canvas.height`.

**Tests** — `pl-zoom`, about 15 checks:
- dragging Bass's header edge down 30 px makes its row 72 px (from 44, snapped) (red first: `FAIL Bass's header has
  an edge to drag`); its clip draws 72 px tall; the next track's header moved 28 px down;
- ⌘↓ with Kick selected: 72; ⌘↑: 44; with none selected: every track 72;
- double-click: 44; an automation lane dragged to 64;
- M, S and the meter at the same offsets from the top (`s8-mute`'s positions);
- close and reopen: the heights are back; `view.json` `heights`.

**Moves:** `s8-mute` (header positions), `s8-meters`, `layout`, `s8-clips` (marquee).

### B3. Follow the playhead — S

**Sees and does**
- Transport ▸ Follow Playhead (⇧⌘F, Ctrl+Shift+F), and the Follow toggle in the ruler's cluster (lit while on;
  `ArrowRightFromLine`). Tooltips: "Follow the playhead (⇧⌘F)" / "Not following the playhead (⇧⌘F)".
- On: while playing, when the playhead passes 85% of the view, the view pages so it sits at 15%. Off: the view stays.
- Scrolling by hand while playing pauses it, until play starts again or the playhead is back in view (Q11).
- Kept per song: `local.follow`.

**Web:** `SongTimeline.tsx` (`:345-350` becomes the paging), `lib/zoom.ts` (the flag), `canvas.follow`.

**Tests** — `pl-zoom`, about 8 checks: play from beat 60 at 48 px a beat: the view pages; off (the toggle, then ⇧⌘F
via `chord()`): it stays (red first: `FAIL the Follow button is there`); a wheel scroll while playing: no paging
until play again; reopen keeps off.

### B4. Repeat a clip — S

**Sees and does**
- The clip menu (`SongTimeline.tsx:1706-1726`) gains Repeat… after Duplicate. It opens a small popover at the clip:
  "Repeat [3] more times" and [Repeat]; Return repeats, Esc closes. Works on several selected clips and on a marquee's
  bars (each copy after the last, as Duplicate places one).
- One undo step, no toast (design-8b §2.4); the copies are selected.

**Back end:** `crates/editor/src/clips.rs` `Gesture::Duplicate` gains `#[serde(default)] times: Option<u32>` (1 to 64,
else refused: "Repeat 1 to 64 times"); `apply` places each copy after the last, each with a fresh id. `m2 clips` takes
it with no change (it sends the gesture as JSON). `web/src/lib/clips.ts`: the type.

**Tests**
- Rust `clips_tests.rs`: `duplicate_three_times_puts_three_copies_after` (red first: `times` unknown field, then
  one copy); `repeat_of_a_marquee_range_keeps_its_bars`; `sixty_five_times_is_refused`.
- Suite **`pl-arrange`** (kind `fix`, new, about 40 checks *estimate*, with B5), after `pl-zoom`: the clip menu has
  Repeat… (red first); 3 makes three Bass clips after the first, one undo step takes all three back; lint: the popover.

**Moves:** `s8-clips`, `s4-clips`.

### B5. Markers — M

**Sees and does**
- On the ruler's top half: a 1 px `fg-faint` tick at each marker, its name in `text-xs` `fg` on `chrome`, cut where
  the next marker starts (design-8b §7.6). It covers bar numbers under its name.
- Add: the ruler's right-click menu "Add marker at bar 9" (`SongBar.tsx:518` `RulerMenu`), at the clicked bar line;
  Transport ▸ Add Marker, at the playhead's bar. It is called "Marker 1" (the next free number) and opens as a field
  with a short list under it: Intro, Verse, Pre-chorus, Chorus, Bridge, Drop, Outro (Q13). Return keeps what is typed.
- Use: a click seeks there; a double-click renames; a drag moves it (snapped; ⌘ held: free, Q14); its right-click menu:
  Rename, Delete, Loop to next marker (the loop from it to the next marker, or the song's end).
- Transport ▸ Previous Marker ⌥←, Next Marker ⌥→ (Alt+Left / Alt+Right): move the start point (as a ruler click).
- Each change is one undo step, no toast. Its name in the Undo tooltip: "Undo Chorus at bar 17", "Undo moving
  Chorus".
- Refused while `layout.json` doesn't read: "layout.json doesn't read: fix it, or reset the layout" (design-8b §2.1).
  Two markers on one beat: "A marker is at bar 17 already".

**Back end**
- `crates/editor/src/markers.rs`: `pub fn set(s: &mut Session, markers: Vec<Marker>) -> Result<Edited, EditError>`: the
  whole list; checks (beats ≥ 0 and finite, one per beat, names one line of 1 to 40 characters); sorted; written into
  `Layout.markers` and `Layout::canonical()`; one layout-only Tx (`layout.rs` `write_op`, `Expect::Identical`, as
  `layout_tx` does; it is an undo step with no revision, `tx/mod.rs` "Only layout.json written"). The label from the
  difference: `("", "Chorus at bar 17")`, `("Chorus", "Big Chorus")`, `("Chorus at bar 17", "Chorus at bar 25")`,
  `("Chorus", "")`.
- `crates/server/src/graph/markers.rs`: `Graph::markers(session, list)`. `routes_view.rs`:
  `POST /api/graph/markers {session, markers: [{beat, name}]}`, a write. The watcher already republishes the layout
  when `layout.json` changes (`graph.rs:1606-1643`).
- `web/src/lib/markers.ts`: the call; the next free "Marker N"; prev and next.

**Tests**
- Rust `crates/editor/src/markers_tests.rs`: `markers_are_written_sorted_into_layout_json`,
  `a_second_marker_on_a_beat_is_refused`, `a_name_of_two_lines_is_refused`, `markers_keep_unknown_keys_and_the_key`,
  `undo_puts_the_old_list_back`, `an_unreadable_layout_refuses` (each red first against the stub).
- `pl-arrange`: the ruler menu has Add marker (red first); a marker at bar 5 is in `layout.json` and its field is open;
  typing "Chorus" and Return names it; picking "Verse" from the list names the next; a drag to bar 9 writes beat 32; ⌘
  held: beat 33.5; a double-click renames; Delete; ⌥→ goes to the next marker, ⌥← back; a click seeks; Loop to next
  marker loops bars 5 to 9; Undo of each; a hand-written `layout.json` with a marker shows it; an unreadable one
  refuses with the words; a PC page says "Alt+→"; lint: markers on the ruler, the field with its list.

**Moves:** `s8-songbar` (the ruler's menu and marks), `layout` (layout.json), `s8-tracks` (reorder writes keep markers).

**Files (all of B):** `SongTimeline.tsx`, `TrackHeader.tsx`, `SongBar.tsx` (`RulerMenu` and `MarkPopover` only),
`GraphCanvas.tsx` (if follow needs the transport), `web/src/lib/zoom.ts`, `heights.ts`, `markers.ts`, `clips.ts`,
`layout.ts`, `crates/editor/src/clips.rs`, `clips_tests.rs`, `markers.rs`, `markers_tests.rs`,
`crates/server/src/graph/markers.rs`, `routes_view.rs`, `e2e/pl-zoom.mjs`, `e2e/pl-arrange.mjs`.

---

## 5. Lane C: Songs and tracks — `wt/pl-c`, branch `polish-c`

Order: C5 first (its keys serve every new dialog), then C1, C2, C3, C4.

### C5. The delete question's keys — S

**Sees and does** (as the export question since `af99eec`, `ExportAsk.tsx:47-66`):
- Return presses the button with the focus ring (Delete until Tab moves it). Space on a button presses it; anywhere
  else in the dialog it does nothing. Tab and ⇧Tab go round the dialog's buttons. No key reaches the page (Space
  doesn't play, M doesn't mute). Esc cancels. Delete keeps the focus at first (Q30).

**Web:** `web/src/lib/modal.ts` (new): `useModalKeys(root, { onEscape, onDefault })`, the logic of `ExportAsk.tsx:47-66`.
`ExportAsk.tsx` uses it (no change in what it does); `DeleteDialog.tsx` uses it; so do C3's and C4's dialogs.

**Tests** — suite **`pl-tracks`** (kind `fix`, new, about 30 checks *estimate*, with C4), after `s8-tracks`:
Kick's delete (it feeds Bass's duck) opens the question; ⇧Tab to Cancel, then Return: nothing deleted (red first:
`FAIL Return on Cancel deletes nothing: kick.ts is gone`); Space with the dialog open doesn't play; M doesn't mute
behind it; Tab goes round two buttons; Return on Delete deletes Kick and the duck; Undo.

**Moves:** `s8-mute` (the export question, now through `modal.ts`), `s8-tracks`.

### C1. View settings kept — S

**Sees and does:** the grid and snap of the timeline and of Notes, and the audition toggle, are as you left them when
the song opens again, also after quitting the app (Q16). B's zoom, follow and heights, and D's hidden lanes, use the
same store.

**Web:** `grid.ts` keeps each view's grid in `local.grid` (`{song: {pick, triplet, on}, notes: {…}}`) instead of
`localStorage` (`grid.ts:117-159`); `audition.ts` keeps `local.audition` (`:25`, `:45`, `:154`). No reading of the old
`localStorage` keys (no backwards compatibility). `e2e/s8-clips.mjs:106-110` read the roll's grid from the chip, not
`localStorage` (only those two helpers; B edits other lines if zoom needs it).

**Tests** — suite **`pl-projects`** (kind `none`, new, about 50 checks *estimate*, with C2 and C3), after `newsong`.
It starts its own servers (`E2E_HEADLESS`) with the run's scratch HOME, on a copy of first-song:
- server 1: the roll's grid set to 1/8, the timeline's snap off, audition off; server 1 stopped;
- server 2 (another port, so an empty `localStorage`): all three are as set (red first: `FAIL the roll's grid is 1/8
  after a new launch: 1/16`); `view.json` holds them.

**Moves:** `s8-clips`, `s8-audition`, `notes`.

### C2. Recent songs — M

**Sees and does**
- **Start screen:** under the three buttons, "Recent" and a 560 px column of up to 20 rows (taste SS-2), 44 px each:
  the song's name (`text-base` 500) and "124 BPM · opened yesterday" (`text-xs` `fg-muted`); the folder in its
  tooltip. A click opens it. Return opens the newest. A folder that is gone: its name in `fg-faint`, "Moved or
  deleted", and an × that forgets it. No thumbnail (left).
- **File ▸ Open Recent ▸** (the app): up to 10 song names, a line, Clear Menu. Picking one asks about unsaved code
  first, as Open does (`actions.rs` `keep_code`), then opens it.
- **At launch** with no folder given: the newest song opens (Q18). If the app closed while opening it last time,
  it starts on the start screen with a toast: "music2 closed while opening Night Drive last time, so it didn't open it
  again."
- Opening a song by any way (Open, New Song, an example, Open Recent, the start screen, Save As) puts it first.

**Back end**
- `crates/server/src/recent.rs`: `Recent { file: PathBuf }`: `list()`, `opened(dir)` (first, no duplicates, 20 at
  most), `forget(dir)`, `clear()`, `reopening(dir)` / `reopened()` (the crash guard), `to_reopen() -> Option<PathBuf>`.
  `~/.music2/recent.json`: `{"songs": [{"path": "/abs", "opened": "2026-10-17T09:00:00Z"}], "reopening": null}`,
  written whole through a temp file and a rename. Each listed song's name and first tempo are read from its
  `music2.toml` and `data/arrangement.json` when listed.
- `crates/server/src/lib.rs`: `ServeOptions.recent: Option<PathBuf>` (the file; None in tests, so no test ever writes
  the real `~/.music2`); `AppState.recent`; `AppState::opened(dir)`.
- `routes.rs`: `graph_open` and `project_new` call `state.opened(dir)` after a successful open (the only lines C
  changes in `routes.rs`). `routes_project.rs`: `GET /api/project/recent`, `POST /api/project/recent/forget {path}`.
- `crates/server/examples/headless.rs`: `recent: $HOME/.music2/recent.json` (the suites' scratch HOME).
- Desktop: `main.rs` passes `~/.music2/recent.json`, and with no folder given opens `to_reopen()` off the main thread
  with the guard (`plugin_host::open_off_main`); `plugin_host::open_project` records the open; `menu.rs` builds Open
  Recent from the list (a pure function of the list, tested) and the app rebuilds the menu when the list changes (the
  title thread, `main.rs` `keep_title_in_sync`, also watches `recent.json`'s time); `actions.rs` `open_recent:<n>` and
  `clear_recent`.
- Web: `web/src/lib/projects.ts` (the calls), `StartScreen.tsx` (the list).

**Tests**
- Rust `recent.rs` tests, in a temp folder (each red first against the stub): `opened_puts_it_first_once`,
  `the_list_keeps_twenty`, `a_crash_while_reopening_is_not_tried_again`, `forget_and_clear`,
  `a_list_that_doesnt_read_is_empty`. `menu.rs`: `open_recent_lists_ten_and_clear_menu`.
- `pl-projects`: three songs made with New Song are listed newest first, with "120 BPM · opened today" (red first:
  `FAIL the start screen lists recent songs`); a click opens one; Return opens the newest; a folder removed by the
  suite shows "Moved or deleted" and its × forgets it; `recent.json` holds them; lint: the start screen with the list.
- The desktop's reopen and Open Recent: the End's Linux app run on `:96` (two launches with `KEEP_HOME`) and the Mac run.

**Moves:** `start`, `newsong`, `fx-fixes` (`none` kind: the start screen).

### C3. Save As, and Rename with the folder — M

**Sees and does**
- **File ▸ Save As… (⇧⌘S, Ctrl+Shift+S):** a dialog "Save a copy of Night Drive", a name field ("Night Drive 2",
  selected), the folder it goes in ("in ~/Music/music2", with Choose Folder… in the app, as New Song has), [Cancel]
  [Save Copy]. Unsaved code in the editor is asked about first (as `closeSong`, `GraphCanvas.tsx:304`). Then the copy
  opens; a toast: "Now working on Night Drive 2. Night Drive is kept as it was." Allowed in a read-only window (it only
  reads the song).
- **File ▸ Rename Song…:** a dialog "Rename Night Drive": the name field; a checkbox "Also rename its folder" (on;
  hidden while the window is read only); while it is on, a line: "The song closes and opens again. Undo starts over."
  [Cancel] [Rename]. The name is one undo step (the title builder); the folder is renamed after it and the song opens
  again from there. Refused with words: "A folder called Summer is already there", "This song is open in another
  window".
- The top bar's double-click on the name stays the quick rename of the name only (Q20).

**Back end**
- `crates/server/src/project.rs`: `save_as(graph, name, parent) -> Result<Value>`: `create(parent, name, dir)` (copies
  without hidden files: no `.music2/`, no `.git`; writes `.gitignore` and the name), then opens it; on the actor so no
  edit lands half way. `rename(graph, name, folder: bool)`: the title Tx (`song::title`); with `folder`, closes,
  `fs::rename` within the parent (refusing a taken name in any capitals, as `taken` does), opens the new path; the
  recent list follows (`replace(old, new)`).
- `crates/server/src/graph/projects.rs`: what needs the session (close and open again).
- `routes_project.rs`: `POST /api/project/save-as {session, name, dir?}`, `POST /api/project/rename {session, name,
  folder}`.
- Web: `SaveAsDialog.tsx` and `RenameSongDialog.tsx` (P0's stubs, filled), `lib/projects.ts`.

**Tests**
- Rust `project.rs` tests (temp folders): `save_as_copies_the_song_without_its_local_folder`,
  `save_as_opens_the_copy_and_leaves_the_original`, `rename_moves_the_folder_and_the_name`,
  `rename_refuses_a_taken_folder`, `rename_in_a_read_only_window_is_refused`.
- `pl-projects`: ⇧⌘S (via `chord()`) opens the dialog (red first); Return makes "First Song 2": its `song.ts` and
  `data/` equal the original's byte for byte, no `.music2/`, its name in `music2.toml`, and it is the open song; the
  original is unchanged; both are in Recent. Rename Song… with the box on: the folder and the name change and the song
  is open from the new folder; with it off: only the name; a taken name is refused with the words. Space and M don't
  reach the page from either dialog. PC page: "Ctrl+Shift+S". Lint: both dialogs.

### C4. Change a track's ID — M

**Sees and does** (design-8b §11.4, taste.md:120):
- The Code view's file list: right-click a file that holds a track's `track()` call: "Change Bass's track ID…" (one
  row per track in that file).
- The dialog: "Change Bass's track ID"; a mono field (`data-code`) holding `bass`; help in `text-sm` `fg-muted`: "The
  track ID names this track in code and files its data (data/bass/). Changing it moves both, as one change you can
  undo."; it checks as you type: "lead vox isn't an ID: IDs are lowercase letters, digits and hyphens, such as
  lead-vox", "low is taken: another track or its data has that ID"; [Cancel] [Change ID].
- After: a toast "Bass's ID is low now" [Undo]. The track keeps its name, colour, mute, solo, height and hidden lanes.
  Its sounds restart (Q21). No UI word says "key" for an ID.

**Back end**
- `crates/editor/src/rekey.rs`: `pub fn rekey(s, module, key) -> Result<Edited, EditError>` (design-8b §11.1):
  - checks: `ready`; the new key is valid and not `tracks::taken` (P0 made it `pub(crate)`);
  - ops: a `TextEdit` of the key string in the `track()` call; a `TextEdit` of the mixer channel's key when it equals
    the old key ("bass → low (and its mixer channel)"); a `Rename` of `data/bass/` to `data/low/` when it exists; a
    `Write` of `layout.json` with the key renamed in `order` and `tracks` (when it reads: else left out, R15), done
    with `Layout`'s public fields in `rekey.rs`, not in `layout.rs`;
  - the file `tracks/bass.ts` and the binding keep their names (§11.1 step 3);
  - `Expect::Confined`: the track file's key string, `song.ts`'s mixer call when the channel changes, every lane of
    the track under both keys, `modules: [Rekeyed { bass, low }]`;
  - the assertion: the delta's modules are exactly `[Rekeyed]`; every lane plays the same clips; no `data.unclaimed`.
- `crates/server/src/graph/projects.rs`: `Graph::rekey(session, revision, module, key)`; after the commit,
  `.music2/view.json`'s `heights` and `hidden` entries follow the new key (overlays already follow `Rekeyed`,
  `actor.rs:808`).
- `routes_project.rs`: `POST /api/graph/track/key {session, revision, module, key}`, a write.
- Web: `TrackIdDialog.tsx` (new), `CodeView.tsx` (`FileList`'s context menu, `:441-488`), `lib/projects.ts`.

**Tests**
- Rust `crates/editor/src/rekey_tests.rs` (design-8b §11.5, without Restore and without the "after step 7" two):
  `rekey_moves_code_data_and_layout`, `the_channel_key_follows_only_when_it_equals`, `a_taken_key_is_refused`,
  `an_invalid_key_says_what_an_id_is`, `undoing_a_rekey_puts_every_byte_back` (red first against the stub).
- `pl-tracks`: right-click `tracks/bass.ts` shows the row (red first); "lead vox" shows the words; `low` renames
  `track("low"…)` and the channel `low:`, moves `data/bass/` to `data/low/`, and `layout.json`'s order says `low`;
  Bass keeps its name, colour, mute and height; Undo puts every byte back; the header menu has no ID row; no visible
  text says "key" next to an ID.

**Moves:** `code`, `s8-tracks`, `s8-mute` (the mute follows), `attention` (`data.unclaimed` words).

**Files (all of C):** `web/src/lib/modal.ts`, `projects.ts`, `grid.ts`, `audition.ts`; components `DeleteDialog.tsx`,
`ExportAsk.tsx`, `StartScreen.tsx`, `SaveAsDialog.tsx`, `RenameSongDialog.tsx`, `TrackIdDialog.tsx`, `CodeView.tsx`,
`NewSongDialog.tsx` (only if the folder chooser is shared); `crates/server/src/recent.rs`, `lib.rs`, `project.rs`,
`routes.rs` (two handlers), `routes_project.rs`, `graph/projects.rs`, `graph/view_file.rs` (a rename helper),
`examples/headless.rs`; `crates/editor/src/rekey.rs`, `rekey_tests.rs`; `apps/music2-desktop/src/main.rs`, `menu.rs`,
`actions.rs`, `plugin_host.rs`; `e2e/pl-projects.mjs`, `pl-tracks.mjs`, `s8-clips.mjs` (two helpers).

---

## 6. Lane D: Notes and automation — `wt/pl-d`, branch `polish-d`

### D1. A menu on notes, and note tools — M

**Sees and does**
- Right-click a note (Ctrl-click on a Mac too): it is selected if it wasn't (alone); the menu opens at the pointer.
  Right-click empty grid: the same menu, for every note of the clip. Its first line says what it acts on: "3 notes" or
  "All 12 notes" (Q25).
- Rows, with their keys:

  | Row | Key |
  |---|---|
  | Up a semitone / Down a semitone | ↑ / ↓ |
  | Up an octave / Down an octave | ⇧↑ / ⇧↓ (Shift+Up / Shift+Down) |
  | — | |
  | Quantize to 1/16 (the roll's grid) | Q |
  | Legato | — |
  | Humanize | — |
  | Swing: Light · Medium · Heavy (a segmented row) | — |
  | — | |
  | Duplicate | ⌘D (Ctrl+D) |
  | Delete | ⌫ (Backspace) |

- Q is new in the roll: it quantizes the selection, or every note, to the roll's grid.
- Each row is one lane write, so one undo step, named "Transpose Bass notes", "Legato on Bass notes", "Humanize Bass
  notes", "Swing Bass notes" (the roll's `send` gets the step's name). Amounts: Q26.
- Read only (the code doesn't compile, a broken lane file): the menu still opens, its rows disabled with the reason.
- The grid's tooltip adds "right-click for note tools". Keyboard: Esc closes the menu and the focus goes back to the roll.

**Back end:** none (the roll's whole-lane write, `NoteView.tsx:253-280`).

**Web:** `web/src/lib/noteTools.ts` (new, pure: `transpose`, `quantize`, `legato`, `humanize`, `swing`, each `(notes,
picked, …) → notes`); `NoteMenu.tsx` (new); `NoteView.tsx` (the context menu handlers, Q, the step names).

**Tests** — suite **`pl-notes`** (kind `fix`, new, about 45 checks *estimate*, with D2), after `s8-audition`:
- right-click a Bass note opens the menu with "1 note" (red first: `FAIL right-click on a note opens its menu`);
  Up an octave: pitch +12 in `data/bass/notes.json`, one undo step; empty grid: "All N notes", Down a semitone moves
  every note;
- Q on notes drawn off the grid: each start on 1/16; Legato: each end equals the next start (a chord's notes
  together), the last unchanged; Humanize: every start within ±10 ms at the song's tempo and velocity within ±0.08 of
  before, and something changed; Swing Medium on a run of 1/16 notes: the second of each pair moves later by 0.05 of a
  beat (60%), the first stays;
- Undo after each restores the file byte for byte; read-only rows are disabled; PC page: "Shift+Up"; lint: the menu.

**Moves:** `notes`, `fixes-notes`, `s8-audition` (the keys and the grid), `pc-keys`.

### D2. The song's key — M

**Sees and does**
- **Top bar:** after the time signature, a field: "Key" in `fg-faint` when none is set; "A minor" when set. Tooltip:
  "The song's key. Rows in the key are lit in Notes. Saved in layout.json." `keepFocus`.
- A click opens a picker (taste.md §2.14 popover): the 12 roots in a 6×2 grid (Q32), Major · Minor (segmented), and
  "No key". A pick writes it and closes. One undo step, no toast: "Undo key A minor".
- **Notes:** while a key is set, rows out of the key are shaded (`hover`), where the black keys' rows are today
  (`NoteView.tsx:984-1013` `Lanes`); rows in the key are plain (Q24). Nothing is locked.
- **The top bar's room:** the song's name keeps clear of the measured left and right groups (Q23); a long name is cut
  sooner by the field's width. At 1100 to 1280 px the status pill still clears the name (`fx-fixes`).

**Back end**
- `crates/editor/src/songkey.rs`: `pub fn set(s: &mut Session, key: Option<SongKey>) -> Result<Edited, EditError>`:
  roots as Q32 (or their sharp spellings), scales `major` and `minor`; written into `Layout.key` and
  `Layout::canonical()` as `{ "root": "A", "scale": "minor" }`; one layout-only Tx, as markers; refused while
  `layout.json` doesn't read. Label `("C major", "A minor")`, or `("", "A minor")`.
- `crates/server/src/graph/songkey.rs`: `Graph::song_key(session, key)`. `routes_notes.rs`:
  `POST /api/graph/key {session, key: {root, scale} | null}`, a write.
- Web: `KeyField.tsx` (P0's mount, filled), `web/src/lib/songKey.ts` (the call, the pitch classes in a key),
  `NoteView.tsx` (`Lanes` shading), `TopBar.tsx` (the measured room) and `SongBar.tsx` (`SongTitle`'s field width only,
  `:432-435`).

**Tests**
- Rust `crates/editor/src/songkey_tests.rs`: `a_key_is_written_into_layout_json`, `no_key_removes_it`,
  `markers_and_unknown_keys_stay`, `a_scale_that_isnt_major_or_minor_is_refused`, `undo_puts_it_back` (red first).
- `pl-notes`: the top bar has a Key field (red first: `FAIL the top bar has a Key field`); A minor writes `layout.json`
  `key`; Keys' roll shades C♯'s row (61) and not C's (60); No key brings back the black keys' shading; Undo; the
  picker's keys (Esc closes, the focus goes back); PC page; lint: the picker open.

**Moves:** `s8-songbar` (the top bar), `fx-fixes` (the name's room), `taste`, `layout` (`layout.json`), `notes`.

### D3. Automation on the control's own scale — S

**Sees and does**
- An automation lane draws on the range its knob declares, widened to hold its points (Q27): a filter's cutoff from
  20 Hz to 20 kHz, not just between the values it has.
- When the knob turns on a log scale (frequencies, times), so does the lane: 1 kHz sits about half way between 20 Hz
  and 20 kHz *(estimate, as the knob)*. A click places a point at the value under it; a drag and Draw mode's paint
  the same.
- A line between two points is drawn as it plays: straight in value, so curved on a log scale.
- A lane whose knob has no range or no log (pan, an amount) draws as today.

**Back end:** none. The view already gives each input the lane that drives it and its knob's `range` and `log`
(`view.rs:177-200`, `:899-963`).

**Web:** `web/src/lib/laneScale.ts` (P0b's extraction): find the input `module` with `lane` equal to this lane, take
its knob's `range` and `log`; `level`, `value`, and `path` that splits each segment into 16 when `log`.

**Tests** — suite **`pl-lanes`** (kind `fix`, new, about 25 checks *estimate*, with D4), after `pl-notes`:
- first-song's Bass Cutoff lane (a `hz` lane driving the bass's cutoff): a point of 1000 Hz is drawn at the height a
  log scale over the knob's declared range gives (the suite reads the range from the view; red first: the lane draws
  it linearly over its points' span); a click at half height writes about the range's geometric middle; the line
  between 200 Hz and 2 kHz has more than 2 vertices; a pan lane (made with Automate on Hats' pan) stays straight;
  Remove automation still writes the playing value (`s8-automation`).

**Moves:** `s8-automation`, `s8-mix` (a fader that follows a lane).

### D4. Hide one automation lane — S

**Sees and does**
- An automation lane's header menu: "Hide this lane" (P0b's row), besides Remove automation and Hide automation.
- Under that track's lanes, a 20 px line: "1 hidden lane · Show" (`text-xs` `fg-muted`, Show is a link button). Show
  brings back every hidden lane of that track. Automate on that control brings it back too (`laneMade`, `lib/events.ts`).
- A (show automation) never shows hidden lanes. Kept per song: `local.hidden: ["bass.cutoff"]`.

**Web:** `lib/hiddenLanes.ts` (P0b's in-memory store, now over `local`, and listening to `laneMade`), `HiddenLanes.tsx`
(the line).

**Tests** — `pl-lanes`, about 10 checks: Hide this lane on Bass's Drop lane removes its row and shows "1 hidden lane ·
Show" (red first: `FAIL Show brings Drop back`, as P0b has no Show); Show brings it back; A keeps it hidden; Automate on
Drop's knob shows it; close and reopen: still hidden; `view.json` `hidden`.

**Files (all of D):** `NoteView.tsx`, `NoteMenu.tsx`, `KeyField.tsx`, `HiddenLanes.tsx`, `TopBar.tsx`, `SongBar.tsx`
(`SongTitle` only), `web/src/lib/noteTools.ts`, `songKey.ts`, `laneScale.ts`, `hiddenLanes.ts`;
`crates/editor/src/songkey.rs`, `songkey_tests.rs`; `crates/server/src/graph/songkey.rs`, `routes_notes.rs`;
`e2e/pl-notes.mjs`, `pl-lanes.mjs`.

---

## 7. File ownership

### 7.1 Who edits what, after P0

| File | Owner | Others |
|---|---|---|
| `Mix.tsx`, `MuteSolo.tsx`, `Meter.tsx`, `TrackMeter.tsx`, `lib/meters.ts`, `lib/overlays.ts`, `RollKeys.tsx`, `lib/rowNames.ts`, `lib/instrument.ts` | A | — |
| `Tracks.tsx` | A (`InstrumentPicker`, `TrackMenu`) | none (P0b moved the delete question out) |
| `core/script/**`, `std/src/drums.ts`, `crates/editor/src/swap.rs`, `note_names.rs`, `crates/server/src/graph/instrument.rs`, `routes_mix.rs` | A | — |
| `SongTimeline.tsx`, `TrackHeader.tsx`, `GraphCanvas.tsx`, `lib/zoom.ts`, `heights.ts`, `markers.ts`, `clips.ts`, `layout.ts` | B | — |
| `SongBar.tsx` | B (`RulerMenu`, `MarkPopover`) | D (`SongTitle`'s field width only) |
| `crates/editor/src/clips.rs`, `markers.rs`, `crates/server/src/graph/markers.rs`, `routes_view.rs` | B | — |
| `CodeView.tsx`, `StartScreen.tsx`, `ExportAsk.tsx`, `DeleteDialog.tsx`, `NewSongDialog.tsx`, the new dialogs, `lib/modal.ts`, `projects.ts`, `grid.ts`, `audition.ts` | C | — |
| `crates/server/src/lib.rs`, `project.rs`, `recent.rs`, `routes_project.rs`, `graph/projects.rs`, `graph/view_file.rs`, `graph/actor.rs`, `examples/headless.rs`, `crates/editor/src/rekey.rs` | C | — |
| `apps/music2-desktop/src/*` | C | — |
| `NoteView.tsx`, `NoteMenu.tsx`, `KeyField.tsx`, `HiddenLanes.tsx`, `TopBar.tsx`, `lib/noteTools.ts`, `songKey.ts`, `laneScale.ts`, `hiddenLanes.ts` | D | — |
| `crates/editor/src/songkey.rs`, `crates/server/src/graph/songkey.rs`, `routes_notes.rs` | D | — |
| `core/runtime/**`, `core/play/**`, `core/song/**`, `core/session/**`, `crates/gates/**`, `apps/m2/**` | nobody | 8a works in the first two; nothing here needs any of them |

### 7.2 The shared files, and the rule for each

| File | Rule |
|---|---|
| `web/src/App.tsx` (keys, chords, `appCommand`) | **P0a only.** It adds every new key, chord and command, each calling a `canvas` hook or a stub file's function. Lanes fill the hook or the stub in their own files. A lane that needs another key asks the lead, who adds it in `polish` |
| `web/src/lib/shell.ts` | P0a only (the hook declarations) |
| `apps/music2-desktop/src/menu.rs` | P0a adds every static row and its test; after that **lane C only** (Open Recent, a submenu built from the list). A row whose feature doesn't land is taken out by the End |
| `web/src/components/Shortcuts.tsx` (the ⌥K sheet) | A key row lands with its feature (principle 7, `Shortcuts.tsx:7-8`). Each lane appends rows at the end of its own group only: B to View and Transport; C to File; D to Edit; A has no keys. Rows go at the end of each group, so two lanes never edit neighbouring lines |
| `e2e/run.mjs` `SUITES`, `e2e/README.md`'s table | Each lane adds its rows right after its anchor: A after `s8-mix` (`pl-mix`, `pl-sounds`); B after `s8-automation` (`pl-zoom`, `pl-arrange`); C after `newsong` (`pl-projects`) and after `s8-tracks` (`pl-tracks`); D after `s8-audition` (`pl-notes`, `pl-lanes`). A lane that changes another suite's count edits only that suite's row |
| `e2e/taste.mjs` | Nobody after P0b. New states are linted in the new suites, through `e2e/lintlib.mjs` |
| `e2e/s8-clips.mjs` | C: the two helpers at `:106-110`. B: lines zoom breaks, if any. Different lines |
| `web/src/lib/graph.ts` | P0a only (types, `request`). Lanes add calls in their own `lib/` file; a new type they need goes in that file by `declare module "./graph"`, as `graph8b.ts` does |
| `crates/server/src/routes.rs` | P0a (the router merges); C (only `graph_open` and `project_new`, C2) |
| `crates/server/src/graph.rs` | P0a only (the `mod` lines). 8a changes one function there; nobody else here |
| `crates/editor/src/lib.rs`, `layout.rs`, `timeline.rs`, `tracks.rs` | P0a only |
| `docs/designs/step8/build-notes-polish.md` | Each part writes only under its own heading (§1.6) |
| `docs/gaps.md`, `README.md`, `docs/architecture.md`, `docs/designs/step8/taste.md`, `HANDOFF.md`, `e2e/README.md`'s prose | **The End only**, from each slice's "For gaps.md" and "Contract" lines. Lanes never edit them |

---

## 8. Order, by day *(estimates)*

| When | Lead | Lane A | Lane B | Lane C | Lane D |
|---|---|---|---|---|---|
| Day 1, morning | P0a, then the worktrees; P0b | reads; writes `pl-mix` (red) | reads; writes `pl-zoom` (red) | reads; writes `pl-tracks` (red) | reads; writes `pl-notes` (red) |
| Day 1, afternoon | merges main when it moves; answers asks | A1, A2 | B1 | C5, C1 | D1 |
| Day 1, evening | | A3 | B2, B3 | C2 | D2 |
| Day 2, morning | | A4; then helps the End (the Linux app checks) | B4, B5 | C3 | D3, D4 |
| Day 2, midday | merges each lane into `polish` as it finishes: A, D, C, B (smallest first), `rerere` on; the merge checks (§9) | done | B5 ends | C4 | done |
| Day 2, afternoon | The End (§9) | | | | |

If a lane is late at Day 2 midday, its unfinished slice waits; the End lists it in `docs/gaps.md`. If something
turns out large, build the part that fits and list the rest as left.

---

## 9. The End (lead)

1. **Merge** each lane into `polish` (merge commits, `rerere`), then `main` if it moved. After all four: the web build;
   every browser suite (`lane-suites.sh pl <out> all`, with `CLAP_PATH` for `live3b`, `run.mjs:30-37`); `cargo test
   --locked --no-fail-fast` and clippy on the crates any lane touched (`m2-script`, `m2-editor`, `m2-server`, `m2`,
   `music2-desktop`, and `m2-session` for the rekey assertion).
2. **Review** with a skeptic, two lenses (how it behaves; tests, docs and taste), as 8b-ui's End did. Each finding: a
   fix with a check that fails before it, its own commit.
3. **Docs** (one commit): `docs/gaps.md` (#1, #3, #4, #6, #9, #10, #12, #14, #15, #17, #20, #23, #28, and the Q7
   loose end: done or what's left; new rows for what this track left); `README.md` (what works); `docs/architecture.md`
   (FMT-19: `markers` and `key`; the HOME table and FMT-20's `view.json` keys; ID-17 and WRITE-10: the track ID change);
   `taste.md` (§2.12 the follow icon; §3.3 the Key field; SS-2 built without its thumbnail); `e2e/README.md`'s rows;
   `HANDOFF.md` (what to try on the Mac, by ear: zoom with a pinch, Open Recent, reopen at launch, ⇧⌘S).
4. **The full check once:** `TW_WAIT=3600 $SCRATCH/tw check-polish 'cd $SCRATCH/wt/pl &&
   CARGO_TARGET_DIR=$SCRATCH/s8c/target-pl M2_CHECK_OUT=$SCRATCH/check-polish ci/check.sh'`. Renders must
   match `e2e/renders.sha256` (nothing here changes sound).
5. **The Linux app on `:96`** (never a real screen): Ctrl+= and Ctrl+− zoom; Ctrl+Shift+S opens Save As; Alt+→ goes to
   the next marker and doesn't navigate; Open Recent lists songs; a second launch with `KEEP_HOME` reopens the song.
6. **Pictures** (§1.7), in `$SCRATCH/app-shots/polish-<sha>/` with `captions.txt`: the Mix view playing with a mute
   and meters; Change instrument's list; a drum kit in Notes with names; the timeline zoomed in with a tall Bass and
   markers; the start screen with Recent; the note menu; the key picker and the shaded roll; a log-scale cutoff lane.
7. **Clean up:** each lane's `target-<lane>` and `bin/<lane>-*` (the lanes do it), then `target-pl` and `bin/`; the
   lane worktrees (`git worktree remove`), keeping their branches.

---

## 10. Cut, and left for later

| What | Why |
|---|---|
| Meters after the fader on strips | Needs taps at each channel's end in `core/runtime` (`taps.rs`, `lib.rs:1871`), which 8a is changing. About a day after 8a lands |
| Curves between automation points | A field in the lane format (`core/song`) and curve interpolation in `core/runtime`: not simple (Q28) |
| A picture of each song in the recent list (taste SS-2) | Reads every listed song's lanes at start; later |
| Drum rows folded to the drums used (taste NT-5), and the roll's vertical zoom | More work in the roll than names; later |
| Restore (DIAG-6) and keeping a renamed track's instruments (design-8b §11.2, §11.3) | Restore is a second fix in the status popover; the instruments need the engine pass in `core/session` |
| Opening a song by dropping its folder on the window or the Dock (#28) | Not asked for here; small, later |
| `m2` commands for change instrument, markers, key, track ID, save as | `apps/m2` is 8a's area this round (gaps #37) |
| Sections (copy a chorus across tracks, insert bars) | gaps #23's rest: large |
| Transposing clips from the timeline | Notes covers it: with nothing selected, a tool acts on the whole clip |
| Copy and paste notes between clips | Not in this track's list (#15's rest) |
| The End flag on the ruler (TL-12) | Not in this track's list; small, later |
| Scale lock, chords, arpeggiator | Scale help (#16) |
| Settings that hold across songs (a user who always wants audition off) | Q16 keeps them per song; a Settings window is #31 |

---

## 11. Not checked

- muda's accelerator names for `=` and `-` (P0a's new test settles them).
- What a trackpad pinch sends in WebKitGTK; whether WKWebView needs `gesture*` events with `preventDefault` to keep the
  page from zooming (the Mac run checks pinch).
- Whether Alt+← makes WebKitGTK go back in the app (P0a prevents it; the End's Linux run checks).
- The `std:drum` kernel's exact note map (A4 reads it before naming).
- How the IR names the frame a notes lane feeds (A4 works it out from `view.rs`'s walk, `:899-940`).
- Whether the title thread can rebuild the native menu while a menu is open (C2; the Mac run).
- Whether `fx-fixes` still passes with the Key field at 1100 px before D measures the groups.
- Where the app's window sits on the Xvfb screen, for the pictures' clicks (measured on the first shot).

---

## Attack

An attack agent read this plan against the code at `29430e8` (read only, nothing built or run). Each finding has an
id (X1…). The Decisions after it answer every one, and say which of Q1 to Q34 change. **Both sections override the
rest of this file.** Note: `main` has moved to `d7207d2` since (HANDOFF and build notes only).

### Keys

| # | Finding | Evidence |
|---|---|---|
| X1 | ⌥← and ⌥→ clash in Notes. The roll's arrow keys don't exclude ⌥ and stop the event, so with notes selected ⌥← nudges notes and never reaches Previous Marker | `NoteView.tsx:366-383` (`arrows[e.key] && picked.length && !cmd`) |
| X2 | Edit ▸ Quantize ignores the roll. The command runs only the timeline's `canvas.quantize`. With D1's Q in the roll, the menu row (and its hint "Q") still quantizes timeline clips, or says "Only notes can be quantized" while the roll has the focus. `App.tsx` is P0a-only | `App.tsx:334-336`, `SongTimeline.tsx:829-838` |
| X3 | ⌘= and ⌘− with the roll focused zoom the timeline, though the roll has its own zoom. taste.md §5.3: where the focus is decides what a chord does (as ⌘D). Q33 rules only on Z, ⌘↑ and ⌘↓ | `NoteView.tsx:203-228` |
| X4 | ⌘+ (⇧⌘=) does nothing in the app: the menu's `CmdOrCtrl+=` doesn't match it, and the page ignores chords in the app | `App.tsx:115` |
| X5 | Monaco's ⌘↑ and ⌘↓ are per OS: `cursorTop`/`cursorBottom` on a Mac, but Ctrl+Up/Down are `scrollLineUp`/`scrollLineDown` on Linux. "The two names" must differ by OS, as `delete_track`'s do | `CodeView.tsx:24-32` |
| X6 | PC words. The tests expect "Shift+Up", "Alt+Left", "Ctrl+Down". The app spells keys only through `native.ts`, with arrows as glyphs on both OSes ("Shift+↑"), and the sheet and `pc-keys` read those | `native.ts:20-37`, `Shortcuts.tsx:39-40` |
| X7 | Ctrl-click on a Mac starts a gesture before the menu: WebKit sends `pointerdown` with button 0 and `ctrlKey`, then `contextmenu` | `NoteView.tsx:410-411` |
| X8 | "⌘ held: free" for markers. Clips use `skipsSnap`: ⌘ on a Mac, Ctrl elsewhere | `grid.ts:91` |

### Seams P0 misses (a lane would have to edit another lane's file)

| # | Finding | Evidence |
|---|---|---|
| X9 | `TimelineView` is taken apart field by field where it is published, on purpose ("this no longer compiles"). P0a's `markers` and `key` need that line and the head fields. P0b's `GraphState.local` also needs the published state (`graph.rs`) and its builder (`actor.rs`). Neither file is in P0's list | `actor.rs:671-675`, `graph.rs:222-226` |
| X10 | Private helpers. `markers.rs` and `songkey.rs` need `layout_tx`, `commit` and `unreadable`, which are private. Swap and rekey need more of `tracks.rs` than the five P0a opens: `with_layout`, `layout_write`, `call_of`, `sorted`, `import_edit`, `applied`, `statement_at` | `layout.rs:457-475`, `tracks.rs:231-463` |
| X11 | Mounting. `SaveAsDialog` and `RenameSongDialog` must be mounted (as `<NewSongDialog />` is), not only imported, and `App.tsx` is P0a-only. `HiddenRow` must be placed under a track's lanes, and its 20 px line counted in `songH`, the playhead's and the marquee's sums. `rowsOf` is a pure function outside the component | `App.tsx:171`, `SongTimeline.tsx:253`, `:1015` |
| X12 | `KEYS_W` is used 9 times in NoteView, 3 of them inside `[]` effects. A width per lane needs every use replaced, and the effects must read it through a ref | `NoteView.tsx:188, 201, 207, 226, 686, 688, 784, 791, 925` |
| X13 | `ServeOptions` is built by struct literal in 5 tests, in headless and in the desktop. C's new `recent` field breaks every other lane's new server tests when they merge, and `routes_8b_tests.rs` is nobody's | `routes_8b_tests.rs:46, 181, 228, 254, 288` |
| X14 | Modal keys. C3 and C4's dialogs need C5's `modal.ts`, which lands only in lane C. And `ExportAsk`'s logic is wrong for a dialog with a text field: it stops every key at the window, Tab goes round the buttons only (never back to the field), and Space is blocked outside a button, so "Night Drive 2" can't be typed | `ExportAsk.tsx:43-55` |
| X15 | `view.json` writes race. `view_file::write` reads, changes and renames through one temp name per process. The loop route and P0b's view route run on different threads: one can lose the other's key, and both use `view.json.<pid>.tmp`. `write` can't delete a key (null is stored), and it is `pub(super)` | `view_file.rs:23-35`, `graph.rs:973` |
| X16 | Shared files §7 doesn't list: `ui.tsx`, `native.ts`, `events.ts`, `store.ts`, `e2e/fixlib.mjs`. Two lanes adding a menu primitive or a suite helper there would collide | — |

### Wrong against the code

| # | Finding | Evidence |
|---|---|---|
| X17 | **D3 can't work as written.** An input an automation lane drives has no knob: `knob` is set only "when a number is written there". And first-song's Cutoff lane doesn't drive the bass's cutoff: it feeds `blend(cutoff, wobble, drop)`, which has no Hz range. The lane has its own unit, `hz` here | `view.rs:80-82`, `examples/first-song/tracks/bass.ts:7-11`, `timeline.rs:52-60` |
| X18 | A3 leaves dead imports. Starter's bass → pad drops `{ cutoff: hz(420), drive: 3 }`, but `hz` stays imported. Step 4 removes only the old instrument's name. `sites::unused_imports` exists | `templates/starter/tracks/bass.ts`, `sites.rs:266` |
| X19 | **B5's Add is two undo steps:** "Marker 1" is written, then the typed name is a second Tx. Undo after naming brings back "Marker 1" | B5 |
| X20 | **Rekey's undo doesn't follow `view.json`.** The follow is only in `Graph::rekey`. Undo (or a rename by hand) brings back `track("bass")` while `view.json` says `low`, so heights and hidden lanes are lost. Overlays already follow any `Rekeyed` delta. Also the page's `local` store keeps "bass" and writes it back 300 ms later, and the shell's `selected`, `clips` and `open` are by key | `actor.rs:799-822` |
| X21 | Rename: "This song is open in another window" can't be known. Only the writer holds the lock; read-only windows hold nothing. And the title is a write, so the route is 423 in a read-only window anyway | `graph.rs:1340-1342` |
| X22 | Reopen at launch: "always the newest" also reopens a song the owner closed (⌘W) before quitting | Q18 |
| X23 | Strip meters: A2 puts them "beside the fader's track", but strips with a lane fader or no fader need one too. A shared strip ("Kick + Snare") has two tracks' peaks: the plan doesn't say which | `Mix.tsx:204-210`, `graph.ts:338-359` |
| X24 | Taller tracks: the header centres M and S (`inset-y-0 … items-center`) and stretches the meter (`top-2 bottom-2`). At 28 px a 28 px meter doesn't fit | `TrackHeader.tsx:82-90` |
| X25 | The zoom and follow buttons "at the right end of the ruler": the ruler is as wide as the song, inside the scroller, so its right end scrolls away | `SongTimeline.tsx:1229-1232` |
| X26 | Key shading on a drum kit lane means nothing | D2, A4 |
| X27 | Swing done twice moves notes again, unless it sets notes to their swung place | D1 |
| X28 | The menu shows ↑ ↓ ⇧↑ ⇧↓ beside "All 12 notes", but those keys move only selected notes | `NoteView.tsx:382` |
| X29 | Recent is recorded at three call sites by hand (`graph_open`, `project_new`, `open_project`), but Save As and Rename open songs too. One missed site and the list is wrong | C2, C3 |
| X30 | **P0a's accelerator test needs a new dependency.** Tauri drops an accelerator it can't parse without a word (`.parse().ok()`). `muda` isn't a dependency of `music2-desktop`; adding it changes `Cargo.lock`, which `--locked` refuses. Read in muda 0.20.0: `=` (Equal), `-` (Minus), `Up` and `Down` parse, and map on GTK (`equal`, `minus`, `Up`) and macOS | `tauri-2.12.3/src/menu/icon.rs:34`, `muda-0.20.0/src/accelerator/mod.rs:297, 327, 365`, `accelerator/gtk.rs:123, 153, 190` |
| X31 | **Red first with an old server serves the new std.** The headless binary finds std, the templates and the examples in the tree it was built in (the lane's worktree), unless `M2_STD`, `M2_TEMPLATES` and `M2_EXAMPLES` are set. An old server with the new `drums.ts` fails to compile songs ("editor has no field notes"), so A4's red run fails for the wrong reason | `resources.rs:3-37`, `e2e/run.mjs:15-22` |

### Tests that can't fail, or can't run as written

| # | Finding |
|---|---|
| X32 | D2: "shades C♯ (61) and not C (60)" in **A minor**. A minor's notes are the white keys, so its shading equals today's black-key shading: the check passes before the change |
| X33 | B1 "at 4 px a beat" and B3 "at 48 px a beat" can't be reached by ⌘− and ⌘=: from 24 the steps are 36, 54, 81, 121.5 and 16, 10.7, 7.1, 4.7, 3.2, 2.1 |
| X34 | B2 "drag down 30 px makes 72": 44 + 30 = 74, half way between the 4 px steps 72 and 76 |
| X35 | B2 "M, S and the meter at the same offsets from the top": at 44 px today's centring gives the same offsets, so it passes before the change |
| X36 | A2 "a fader at −30 dB doesn't move the strip's meter": nothing post-fader exists for the strip to read, and two live peaks a moment apart differ anyway. It can't fail red, and it is flaky |
| X37 | P0b writes 300 ms after the last change. A close, Save As, reopen or quit within 300 ms loses the change. C1's and B1's "close and reopen" checks race it |

### Too big for a lane, order, words

| # | Finding |
|---|---|
| X38 | Lane C is the long pole: five slices running to Day 2 midday, the desktop work among them. Lane A is done after A4 (Day 2 morning, "then helps the End"). C4 shares no file with the rest of C except `routes_project.rs` and `graph/projects.rs`, which it doesn't need |
| X39 | Lane B is also full (B1 is close to L: `PPB` is a constant used 49 times, also in functions outside the component such as `usePlayhead` and `Fallback`). B4 is the least needed slice (⌘D already repeats one copy) |
| X40 | P0b blocks B, C5 and D: it edits `SongTimeline.tsx`, `NoteView.tsx` and `Tracks.tsx`, which they can't touch until it lands, and the view store comes in the same commit |
| X41 | Owner words that are jargon or wrong: "Kick's level, before its fader"; "Went over 0 dB"; "Bass's cutoff follows other code"; the Key tooltip "Rows in the key are lit" (Q24 shades the rows not in the key); "Undo starts over"; Quantize, Legato, Humanize and Swing with no help |
| X42 | Routes `/api/graph/key` (the musical key) and `/api/graph/track/key` (the track ID) mix the two meanings taste.md:71 keeps apart |
| X43 | Start screen: "Return opens the newest" fights the app's rule that Return presses the button with the focus |
| X44 | D4's "1 hidden lane · Show" line: shown while the track's lanes are folded too? Not said |
| X45 | The plan sends markers through a new `/api/graph/markers` and the timeline, where design-8b sends them through `/api/graph/layout` and `LayoutView` (`design-8b.md:212`, `:339`). Fine for file ownership, but the End's docs must say so |

---

## Decisions

The lead builds with these. Every Q not named in the last table stands.

### Plan changes

| # | Decision |
|---|---|
| X38 | **C4 moves to lane A, as A5**, after A4. A5 is A's cut if A is late. A5's files: `crates/editor/src/rekey.rs`, `rekey_tests.rs`, `crates/server/src/graph/rekey.rs` (P0a adds `mod rekey;` to `graph.rs`), `POST /api/graph/track/rekey` in `routes_mix.rs`, the follow in `graph/actor.rs` and a rename helper in `graph/view_file.rs` (both now A's), `TrackIdDialog.tsx`, `CodeView.tsx`, `web/src/lib/rekey.ts`. Its checks go in a new suite **`pl-ids`** (kind `fix`, about 25 checks *estimate*) after `pl-sounds`. C keeps `pl-tracks` for C5 only |
| X39 | **B's order: B1, B3, B2, B5, B4.** Repeat is B's cut if B is late at Day 2 midday |
| X40 | **P0b lands in two commits.** P0b-1: the extractions (`RollKeys`, `rowNames`, `laneScale`, hidden lanes, `DeleteDialog`, `lib/modal.ts`, `lintlib.mjs`). P0b-2: the view store. B, C5 and D start code when P0b-1 lands; C1, and B's and D's saving, when P0b-2 does |
| `main` moved | The lead merges `main` (`d7207d2`) into `polish` before P0a |

### Keys

| # | Decision |
|---|---|
| X1 | The roll's arrow keys ignore ⌥ (D, in NoteView). ⌥← and ⌥→ are always the markers' |
| X2 | P0a: `quantize` asks the roll first, as `duplicate` does: `editorHooks.quantize?.()`, which D fills while the roll has the focus |
| X3 | P0a: `zoom_in` and `zoom_out` ask `editorHooks.zoom?.(dir)` first. D fills it with the roll's own zoom (×1.5) while the roll has the focus. Z, ⌘↑ and ⌘↓ stay the timeline's (Q33) |
| X4 | P0a: ⇧⌘= (`+`) zooms in, handled by the page even in the app (no menu row has it) |
| X5 | P0a: Monaco gets `cursorTop` / `cursorBottom` on a Mac and `scrollLineUp` / `scrollLineDown` elsewhere |
| X6 | Every label goes through `native.ts`. Arrows stay glyphs on both OSes: "Shift+↑", "Alt+→", "Ctrl+↓", and "Ctrl+−" for zoom out. The tests expect those words |
| X7 | D: the roll's `pointerdown` ignores Ctrl on a Mac (it is a right-click there) |
| X8 | A marker drag uses `skipsSnap`: ⌘ on a Mac, Ctrl on Linux (Q14) |
| X30 | No new dependency. P0a's menu test checks only the new rows and their places. The names it uses are the ones read in muda 0.20.0 (`=`, `-`, `Up`, `Down`). The real check is the End's Linux app run (§9 step 5), which also presses Ctrl+Up, Ctrl+Down and Ctrl+Shift+F |

### Seams

| # | Decision |
|---|---|
| X9 | P0a edits `actor.rs:671-675` to publish `markers` and `key` in the timeline's head. P0b adds `local` (and `local_stamp`, X20) to the published state in `graph.rs` and `actor.rs` |
| X10 | P0a makes `pub(crate)`: `layout_tx`, `commit` and `unreadable` in `layout.rs`, and every private function in `tracks.rs`. Nothing else changes in them |
| X11 | P0a mounts `<SaveAsDialog />` and `<RenameSongDialog />` beside `<NewSongDialog />` (the stubs render nothing). P0b-1 places `<HiddenRow module>` under each track's lanes and counts its height from `hiddenRowHeight(module)` in `hiddenLanes.ts`, which returns 0 until D. The line shows only while that track's lanes show (X44) |
| X12 | P0b-1 replaces every `KEYS_W` with `keysWidth(lane)`, read through a ref inside effects |
| X13 | P0a adds `recent: Option<PathBuf>` to `ServeOptions` now, derives `Default`, and writes every literal as `ServeOptions { …, ..Default::default() }`. New tests do the same. New Rust tests live in a file named by their module's own `#[path = "x_tests.rs"] mod tests;` (as `clips.rs:1105` does), never by a line in `lib.rs` or `graph.rs` |
| X14 | P0b-1 makes `web/src/lib/modal.ts`: `useModalKeys(root, { onEscape, onDefault })`, moves `ExportAsk` onto it (same behaviour) and fixes it for fields: Tab and ⇧Tab go round every control in the dialog; keys in a text field are the field's (Space types a space); Return in a field presses the default button; Esc cancels; no key reaches the page. C5 then only puts `DeleteDialog` on it. Every new dialog uses it |
| X15 | P0b-2: `view_file::set(dir, changes)` merges a map (null removes a key). It and `write` take one `static` mutex, and the temp name gets a counter. `Graph::set_view` lives in `view_file.rs`. The route accepts only `zoom`, `follow`, `heights`, `hidden`, `grid`, `audition` |
| X16 | Nobody edits `ui.tsx`, `native.ts`, `events.ts`, `store.ts`, `e2e/fixlib.mjs` or `e2e/chrome.mjs`. A lane that needs a new primitive or helper keeps it in its own file |
| X37 | `local.ts` sends what waits at once before a close, an open, Save As, Rename and `pagehide`. Suites wait until `view.json` holds the value before they close or stop a server |

### Behaviour

| # | Decision |
|---|---|
| X17 | **D3 goes by the lane's unit (Q27 replaced).** A `hz` lane draws on a log scale from 20 Hz to 20 kHz, widened to hold every point; a line between two points is cut into 16 pieces. Every other unit draws as today (linear, the server's min and max). `laneScale(lane)` takes only the lane. The test uses first-song's Cutoff lane: a click at half height writes about 632 Hz (√(20 × 20000)); red first, today's linear scale writes the middle of the points' span |
| X18 | Swap also removes every import that only the dropped text used (`sites::unused_imports`). `swap_drops_plain_settings` checks `hz` is gone from the starter's bass |
| X19 | **Add marker is one undo step.** It opens the name field first. The marker is written once, when the name is kept: Return, a pick from the list, or a click away (it is then "Marker N"). Esc adds nothing. The suite checks one Undo after naming takes the marker away |
| X20 | **The follow runs where overlays follow:** in `actor.rs`, on any delta with `Rekeyed` (the rekey, its undo, a rename by hand). It works out the rekeys also when `view.json` has `heights` or `hidden`. The server then bumps `GraphState.local_stamp`, and the page seeds `local` again when the stamp changes (never for its own posts). After a rekey or its undo, `lib/rekey.ts` moves the shell's `selected`, `module`, `clips` and `open` to the new key with `useShell.setState` (no edit to `shell.ts`). Test in `pl-ids`: Bass at 72 px, rekey to `low`, Undo: Bass is still 72 px and selected |
| X21 | Rename Song… is disabled in a read-only window, with the lock's reason. No "open in another window" refusal. The folder name is checked before anything is written. If the folder rename then fails, the name change stays (one undo step) and a toast says why |
| X22 | `recent.json` keeps `"open": true` while the newest song is open; Close Song sets it false. At launch the newest song opens only if it was open when the app quit (and the crash guard allows). New test: `a_song_closed_before_quitting_doesnt_reopen` |
| X23 | Every track strip has a meter, whatever its fader (a number, a lane, or level in code), in the fader's column, the travel tall. A shared strip shows, per side, the louder of its tracks. No meter on a channel no track reaches |
| X24 | 44 px and taller: name, ⋯, M and S stay in the row's top 44 px (centred in it), the meter 28 px tall from 8 px down. Under 44 px: all centred in the row, the meter the row's height minus 8 |
| X25 | The group is `sticky right-0` in the ruler's row, on `chrome` with a 1 px `line` edge on its left. The ruler's click, drag and right-click don't start under it |
| X26 | A lane with drum names is never shaded by the key |
| X27 | Swing **sets** each off-beat note (start within a quarter step of the half-way point of its pair) to its swung place, so doing it twice changes nothing. Notes elsewhere stay. The suite checks Swing Medium twice equals once |
| X28 | With no note selected, Q acts on every note (Q25); the arrow keys still act only on selected notes, so the menu shows the arrow keys only while notes are selected |
| X29 | One `AppState::open_song(dir)` opens and records. `graph_open`, `project_new`, the desktop's `open_project`, Save As and Rename all call it |
| X31 | A red run on an old server also sets `M2_STD`, `M2_TEMPLATES` and `M2_EXAMPLES` to a copy of the old tree: `git worktree add --detach $SCRATCH/s8c/scratch/old-<sha> <sha>`, removed after. `lane-suites.sh` takes `OLD=<folder>` and sets all three |
| X43 | On the start screen Return opens the newest song only when no button has the focus |

### Tests

| # | Decision |
|---|---|
| X32 | D2's test uses **E minor**: F♯'s row (66) is plain and F's (65) is shaded. Today it is the other way round, so it is red first |
| X33 | Zoom is a number from 2 to 192, not only the steps: ⌘= and ⌘− multiply it by 1.5 (then clamp), Z and pinch set any value (Q8). Tests that need an exact zoom write `zoom` into `view.json` before the song opens (4 px a beat for the ruler, 48 for follow), or use a reachable step (54 = 24 × 1.5²) |
| X34 | B2's drag is 28 px: 44 → 72 |
| X35 | B2's offsets check runs on a 72 px row: M's top is 12 px below the row's top, as at 44 px (centred, it would be 26) |
| X36 | A2 instead checks, while first-song plays, that the Hats strip's meter stays within 1.5 dB of the Hats header meter (the same tap) over 10 samples, before and after its fader goes to −30 dB. Red first: the strip has no meter |

### Words and routes (X41, X42, X45)

| Where | The plan said | Build with |
|---|---|---|
| Track strip meter tooltip | Kick's level, before its fader | Kick's level. Its fader doesn't change this meter. |
| Clip dot tooltip | Went over 0 dB. Click to clear. | Too loud: it went over the top. Click to clear. |
| Swap refused | Bass's cutoff follows other code. Change its instrument in code. | Bass's cutoff is set by code here. Change its instrument in the Code view. |
| Key field tooltip | The song's key. Rows in the key are lit in Notes. Saved in layout.json. | The song's key. In Notes, rows not in the key are shaded. Saved in layout.json. |
| Rename dialog line | The song closes and opens again. Undo starts over. | The song closes and opens again. After that, Undo can't go back past this. |
| Note menu tooltips | none | Quantize: "Move the notes onto the grid". Legato: "Make each note last until the next one starts". Humanize: "Change timing and loudness a little, so it sounds played". Swing: "Push every second note a little later, for a bouncy feel" |
| Routes (X42) | `/api/graph/key`, `/api/graph/track/key` | `/api/graph/song-key` (D2) and `/api/graph/track/rekey` (A5). `layout.json` keeps the field name `key` (Q22) |
| Contract (X45) | — | The End's FMT-19 and WRITE-10 lines say markers and the key are written by their own routes, not `/api/graph/layout` |

### The questions this changes

| Q | Now |
|---|---|
| Q8 | Zoom is any value from 2 to 192; the steps are ×1.5 from where it is (X33) |
| Q9 | ⌘= and ⌘− zoom the roll while it has the focus (X3); ⇧⌘= zooms in too (X4) |
| Q10 | The header's layout at each height: X24 |
| Q13 | Add writes once, when the name is kept (X19) |
| Q14 | ⌘ on a Mac, Ctrl on Linux (X8) |
| Q16 | One lock for every `view.json` write; known keys only; writes sent at once before a close (X15, X37) |
| Q18 | Only a song that was open when the app quit (X22) |
| Q20 | Disabled in a read-only window; no "other window" refusal (X21) |
| Q21 | The view settings follow the ID on undo too (X20). Built by lane A (X38) |
| Q24 | Drum lanes are never shaded (X26) |
| Q26 | Swing sets notes to their swung place (X27) |
| Q27 | **Replaced** by X17: by the lane's unit; Hz on a log scale over 20 Hz to 20 kHz |
| Q31 | Sticky at the ruler's right (X25) |
| Q33 | Except ⌘= and ⌘− (X3) |

### Order, by day (changes to §8)

| When | Lead | Lane A | Lane B | Lane C | Lane D |
|---|---|---|---|---|---|
| Day 1, morning | merge `main`; P0a; worktrees; P0b-1; P0b-2 | red suites | red suites; code after P0b-1 | red suites; C5 after P0b-1 | red suites; code after P0b-1 |
| Day 1, afternoon | answers asks | A1, A2 | B1 | C5, C1 | D1 |
| Day 1, evening | | A3 | B3, B2 | C2 | D2 |
| Day 2, morning | | A4 | B5 | C3 | D3, D4 |
| Day 2, midday | merges A, D, C, B as each ends | A5 (track ID) | B4 if time, else left | done | done |
