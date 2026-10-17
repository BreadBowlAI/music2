# Polish: build notes

What each part of the polish track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8c/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix and sounds.

## Lane B

Timeline: zoom, follow the playhead, track heights, markers, repeat a clip. Branch `polish-b`,
worktree `wt/pl-b`.

There was no P0 when this lane started. Lane B built the seams it needs itself: the view store
(P0b-2), its keys and menu rows (P0a's, for its own ids only), in its own files where it could and
in small blocks of the shared ones.

### View settings kept per song (P0b-2's view store)

**In short.** The app can now keep how you look at a song: the zoom, the track heights and so on.
They are kept in the song's own `.music2/view.json`, on this computer only. They come back when the
song opens again.

| What | How |
|---|---|
| Where | `.music2/view.json`, beside the loop it already kept. Never shared, never an undo step |
| Which settings | `zoom`, `follow`, `heights`, `hidden`, `grid`, `audition`. Any other name is refused |
| Read-only window | The settings work, but only in memory: the file is the writer's |
| Two writes at once | One lock in the server, and each write has its own temporary file: no write loses another's |

**What changed**

- `crates/server/src/graph/view_file.rs`: `set(dir, changes)` merges a map in (null takes a key
  out), under one lock with a temporary name per write; `write` (the loop) goes through it.
  `local(dir)` reads the page's keys. `Graph::set_view(session, changes)` lives here.
- `graph.rs`, `actor.rs`: the published project holds `local`, read from `view.json` at the open;
  the state carries it as `local`.
- `crates/server/src/routes_view.rs` (new): `POST /api/graph/view {session, set}`. Not a write.
- `web/src/lib/local.ts` (new): `useLocal(key, fallback)`, `getLocal`, `setLocal(key, value)`,
  `flushLocal()`. Seeded from the state's `local` when the song changes. Sent 300 ms after the last
  change; sent at once before Close Song and on `pagehide` (X37).

**Tests.** Rust, `crates/server/src/graph/view_file_tests.rs`:
`view_settings_merge_and_keep_the_loop` (a set, a null, a refused `loop` and unknown key, another
session refused, kept over a reopen) and `a_read_only_window_keeps_view_settings_in_memory`.
`cargo test -p m2-server --lib view_file`: 2 passed. Clippy of `m2-server`: clean.

**Red first.** With `set_view` a stub that refuses:
`panicked at crates/server/src/graph/view_file_tests.rs:43:95` and `:75:87` (the set is refused).
The old server answers the route 404.

**Left.** Lane C's grid and audition, and lane D's hidden lanes, can use `useLocal` and `setLocal`
with their keys. A song opened from the desktop's Open while a setting waits its 300 ms loses it.

**For gaps.md.** View settings are kept per song (Q16): zoom, follow and heights so far.

**Contract.** FMT-20 and the HOME table: `view.json` also keeps `zoom`, `follow`, `heights`,
`hidden`, `grid`, `audition`, set through `POST /api/graph/view`.

### B1 and B3. Zoom, and follow the playhead

**In short.** You can zoom the timeline in and out. While a song plays, the view moves along with
the playhead. You can turn that off. Both are kept with the song.

| Do this | Mac | Linux | What happens |
|---|---|---|---|
| Zoom in or out | ⌘= / ⌘− (⇧⌘= too) | Ctrl+= / Ctrl+− | Around the playhead when you can see it; else around the middle |
| Zoom around the pointer | pinch, or ⌘-scroll | Ctrl-scroll | The beat under the pointer stays under it |
| Zoom to fit | Z | Z | The selected clips fill the view; with none selected, the whole song. Z again goes back |
| The buttons | right end of the ruler | same | Follow, Zoom out, Zoom in. They stay in view as you scroll |
| Follow the playhead | ⇧⌘F | Ctrl+Shift+F | When the playhead passes 85% of the view, the view jumps so it sits at 15% |

- The zoom goes from 2 to 192 px a beat. Each step is ×1.5. It starts at 24.
- Bar numbers thin out when bars get narrow: every 2nd, 4th or 8th bar under 40, 20 and 10 px.
  Beat lines go under 6 px a beat; bar lines stay.
- The grid's Auto follows the zoom: 1/8 at 24 px a beat, 1/16 at 54.
- Scrolling sideways by hand while it plays pauses following. It starts again when play starts
  again, or when the playhead comes back into view.
- In the Notes roll (lane D's part), ⌘= and ⌘− ask the roll first (`editorHooks.zoom`).

**What changed**

- `web/src/lib/zoom.ts` (new): the zoom and the follow flag, over `lib/local` (`zoom`, `follow`).
- `SongTimeline.tsx`: `PPB` (24, used in 49 places) is the zoom now, read with `useZoom()` in each
  component and `zoomNow()` in the edits. `zoomTo` keeps a beat in place; the lanes keep room for
  it (`spare`), so a zoom near the song's end still holds the beat. A wheel with ⌘ or Ctrl, and
  WebKit's `gesture*` events (with `preventDefault`), zoom around the pointer. `ViewTools`: the
  ruler's group, `sticky right-0` with its own 1 px edge (X25). Following pages on each frame from
  the clock; the lanes are a view wider while it follows, so a page can always put the playhead
  at 15%.
- Keys and rows (P0a's, for lane B's ids): `App.tsx` `chord()` (=, +, −, ↑, ↓, ⇧F) and `key()` (Z,
  ⌥←, ⌥→ with `preventDefault`); `appCommand` cases; `shell.ts` `canvas.zoom`, `zoomFit`,
  `follow`, `height`, `marker`; `graph.ts` `editorHooks.zoom`; `CodeView.tsx`'s two Monaco names
  for ⌘↑ ⌘↓ (X5); `menu.rs` rows Zoom In, Zoom Out, Zoom to Fit, Taller Tracks, Shorter Tracks
  (View, first), Follow Playhead, Add Marker, Previous Marker, Next Marker (Transport), and a test.
  `Shortcuts.tsx`: rows for zoom and follow.
- `e2e/lintlib.mjs`: the taste lint as a function (P0b-1's), for the new suites.

**Tests.** Suite `pl-zoom` (kind `fix`, 43 checks, after `s8-automation`). The menu rows:
`music2-desktop` `zoom_heights_follow_and_markers_have_their_rows` (9 menu tests pass). Moved
suites, all green with their counts: `taste` 39, `s8-clips` 94, `s4-clips` 28, `s8-automation` 45,
`s8-songbar` 82, `layout` 39, `views` 30, `gestures` 30, `pc-keys` 37, `code` 58. Clippy of
`m2-server` and `music2-desktop`: clean.

**Red first** (the page and server of `29430e8`):
`FAIL the ruler has Zoom in and Zoom out`, `FAIL ⌘= zooms in: clip width 766 → 766`, then
`No element found for selector: [data-testid=zoom-in]`.

The buttons are named "Zoom the timeline in" and "Zoom the timeline out" for screen readers: the
Device view's graph has its own Zoom in and Zoom out (`ports` clicks those by name).

**Left.** A trackpad pinch is checked only as Chrome's Ctrl-wheel; the Mac's `gesture*` events
need the Mac run. Follow is a plain menu row, not a ticked one.

**For gaps.md.** Zoom (#9) and follow the playhead are done.

**Contract.** None beyond the view store's.

### B2. Taller tracks

**In short.** You can make a track taller or shorter. Drag the bottom edge of its name box. Or use
the keys. Automation lanes have the same edge, so they are easier to draw on.

| Do this | Mac | Linux | What happens |
|---|---|---|---|
| Drag a header's bottom edge | | | 28 to 160 px, in 4 px steps |
| Double-click that edge | | | Back to the usual 44 px (32 for a lane) |
| Taller / Shorter Tracks | ⌘↓ / ⌘↑ | Ctrl+↓ / Ctrl+↑ | Steps 28, 44, 72, 120: the selected track, or every track when none is selected |

- A track's other notes rows take its height. An automation lane goes from 24 to 160 px.
- In a row 44 px or taller, the name, ⋯, M, S and the meter stay in its top 44 px, as they were.
  In a shorter row they are centred, and the meter is the row's height less 8 (X24).
- Kept per song in `view.json`: `heights: {"bass": 72, "bass.cutoff": 64}`. One at its usual
  height isn't kept.
- The Song's row at the foot keeps its usual heights.
- In the code editor ⌘↑ and ⌘↓ are the editor's own (to the top and foot on a Mac, a line's
  scroll on Linux); in a text field, to its start and end.

**What changed.** `web/src/lib/heights.ts` (new): the store over `lib/local`, the steps and the
snap. `SongTimeline.tsx`: each track's rows take their heights; `HeightEdge`, a 6 px
`row-resize` strip at the foot of a track's header and an automation lane's; `LaneLabel` keeps to
the row's top; `canvas.height` for ⌘↑ ⌘↓. `TrackHeader.tsx`: its parts in a box the row's top
44 px tall (X24).

**Tests.** `pl-zoom` grows to 62 checks: the edge (6 px, at the foot, its tooltip); a 28 px drag
makes Bass 72 px, its clip 64 px tall and Keys 28 px lower; M and S 12 px and the meter 8 px from
the top, the meter 28 tall (X35); kept in `view.json`; a double-click back to 44, and nothing
kept; ⌘↓ and ⌘↑ on Kick; ⌘↑ on Hats to 28, M centred, the meter 20 tall; with none selected ⌘↓
makes every track 72; Bass's Cutoff lane dragged to 64; both kept, and back after a reopen; lint.
Moved suites green: `s8-mute` 152, `s8-meters` 24, `layout` 39, `s8-clips` 94, `s8-tracks` 75,
`s8-automation` 45, `taste` 39.

**Red first** (the page of `fddb62e`): `FAIL Bass's header has an edge to drag: 6 px at its foot`,
then `failed to find element matching selector "[data-height-edge="bass"]"`.

**Left.** The roll's own row height (its vertical zoom) is lane D's and later.

**For gaps.md.** Track heights (#17's thin lanes too) are done.

**Contract.** None beyond the view store's.

### B5. Markers

**In short.** You can put markers on the ruler: Intro, Verse, Chorus and so on. They show where
each part of the song starts. They are saved in the song, so they are shared.

| Do this | How |
|---|---|
| Add one | Right-click the ruler: "Add marker at bar 9". It opens as a field called "Marker 1", with a short list: Intro, Verse, Pre-chorus, Chorus, Bridge, Drop, Outro |
| Name it | Type and press Return, or pick from the list. A click away keeps "Marker 1". Esc adds nothing |
| Go to it | Click it. Or ⌥← and ⌥→ (Alt+← and Alt+→) for the one before or after |
| Move it | Drag it: it lands on the grid. Hold ⌘ (Ctrl on Linux) to place it freely |
| Rename it | Double-click it |
| Its menu | Right-click it: Rename, Delete, Loop to next marker (to the next marker, or the song's end) |

- Each change is one undo step, with no toast. Undo names it: "Undo Chorus at bar 17", "Undo moving
  Chorus", "Undo renaming Verse", "Undo deleting Marker 1".
- A new marker is written once, when its name is kept (X19): one Undo takes it away.
- Two markers on one beat are refused: "A marker is at bar 9 already". The ruler's row says so too.
- While `layout.json` doesn't read, no markers show, and adding one is refused: "layout.json doesn't
  read: fix it, or reset the layout".
- Transport ▸ Add Marker adds one at the playhead's bar.

**What changed**

- `crates/editor/src/markers.rs` (new): `Marker {beat, name}`, `read(&Layout)` (well-formed entries
  by beat), `on_disk(dir)`, and `set(s, list)`: the whole list, checked, sorted, written into
  `Layout.markers` by `canonical()`, as one layout-only Tx with its label from the change. Whole
  beats are written without `.0`. `layout.rs`: `layout_tx`, `commit` and `unreadable` are
  `pub(crate)`.
- `timeline.rs`: `TimelineView.markers`, in `timeline()` and `last_known()`; `actor.rs` publishes
  it in the timeline's head.
- `crates/server/src/graph/markers.rs` (new): `Graph::markers`. `routes_view.rs`:
  `POST /api/graph/markers {session, markers}`, a write (423 read-only), with its own `writes`
  layer, as `routes_8b.rs`.
- `web/src/lib/markers.ts` (new): the call, the next free "Marker N", the one before and after, the
  open field (`useMarkerEdit`), the list drawn until the timeline shows it.
  `web/src/components/Markers.tsx` (new): the flags, their drag, menu and the name field.
  `SongTimeline.tsx`: the ruler draws them; `canvas.marker` (Add, Previous, Next); ⌥→ twice goes on
  to the second marker even before the clock has heard of the first. `SongBar.tsx` `RulerMenu`:
  the Add marker row. `Shortcuts.tsx`: ⌥← ⌥→.
- `e2e/s8-songbar.mjs`: its check of the ruler menu's rows names the third row.

**Tests.** Rust, `crates/editor/src/markers_tests.rs`: `markers_are_written_sorted_into_layout_json`
(and each change's label, no step when nothing changes, no key when none are left),
`a_second_marker_on_a_beat_is_refused`, `a_name_of_two_lines_is_refused` (and empty, 41
characters, a beat before 0; 40 é is fine), `markers_keep_unknown_keys_and_the_key`,
`undo_puts_the_old_list_back`, `an_unreadable_layout_refuses`. `m2-editor` 257 tests and
`m2-server` 220 pass; clippy of both clean. Suite `pl-arrange` (kind `fix`, 44 checks, after
`pl-zoom`). Moved suites green: `s8-songbar` 82, `layout` 39, `s8-tracks` 75, `taste` 39,
`pc-keys` 37.

**Red first.** Rust, with `set` a stub that refuses: all six fail (`markers_tests.rs:18:88`,
`:52:5`, `:67:9`, `:82:56`, `:97:40`, `:118:5`). The suite, on the page and server of `29430e8`:
`FAIL the ruler's menu has Add marker at bar 5: Add tempo change at 5.1 | Add time signature
change at bar 5`.

**Left.** Transport ▸ Add Marker is checked only on the desktop app (no browser suite reaches the
menu bar). A marker keeps no fields but its beat and name. Markers don't move with a tempo or
meter change.

**For gaps.md.** Markers on the ruler (#23's markers) are done; sections (copy a chorus, insert
bars) are left.

**Contract.** FMT-19: `markers` is `[{ "beat", "name" }]`, sorted, one a beat, names one line of 1
to 40 characters, written by `POST /api/graph/markers`, not `/api/graph/layout` (X45). WRITE-10's
route list gains it.

### B4. Repeat a clip

**In short.** You can repeat a clip a few times in one go. Right-click it and pick Repeat…. Say
how many more times. The copies follow one after another.

| What | How |
|---|---|
| Where | The clip menu, after Duplicate |
| The question | "Repeat [3] more times" and a Repeat button. Return repeats; Esc closes |
| How many | 1 to 64. Anything else: "Repeat 1 to 64 times", and the button is off |
| What it makes | Copies right after the clip (or the selected clips, or a marquee's bars), each a new clip with its own ID. They are selected after |
| Undo | One step for all of them: "Undo repeating a Bass clip". No toast |

**What changed.** `crates/editor/src/clips.rs`: `Gesture::Duplicate` gains `times` (1 to 64,
`REPEATS`; one when not given); each copy lands right after the one before. `apps/m2/src/edits.rs`:
`m2 clips duplicate` sends no `times` (one line; `m2` commands for it are left, gaps #37).
`web/src/lib/clips.ts`: the type. `SongTimeline.tsx`: ⌘D and Repeat… share `copies(times)`; the
clip menu's Repeat… row; `RepeatPopover`. `e2e/s8-clips.mjs`: its clip menu check names the new
row.

**Tests.** Rust, `clips_tests.rs`: `duplicate_three_times_puts_three_copies_after` (spans, fresh
ids, the copies play the first one's notes, one undo), `repeat_of_a_marquee_range_keeps_its_bars`,
`sixty_five_times_is_refused` (0 and 65 refused, 64 made). `m2-editor` 260 tests pass, `m2`'s pass;
clippy of `m2-editor`, `m2-server`, `m2` clean. `pl-arrange` grows to 54 checks. Moved suites
green: `s8-clips` 94, `s4-clips` 28, `taste` 39.

**Red first.** Rust, with `times` ignored (one copy): the three fail, `left: [(0.0, 4.0), (4.0,
4.0)]`, `right: [(0.0, 4.0), (4.0, 4.0), (8.0, 4.0), (12.0, 4.0)]`. The suite on the page of
`61bcea4`: `FAIL the clip menu has Repeat… after Duplicate: cut, copy, paste, duplicate, split,
quantize, delete, loop, open`.

**Left.** No key for Repeat… (⌘D repeats once).

**For gaps.md.** Repeat a clip (#23's loop-a-clip-n-times) is done.

**Contract.** The clips route's `duplicate` gesture takes `times` (1 to 64).

### Lane B: the last run

Every browser suite but `live3b` (it needs plugins) ran on lane B's tip, in three batches: all
green with their counts, but `ports` (2 failed: it clicked the timeline's new Zoom in, by the same
name as the graph's). The buttons got their own names; `ports` 46 and `pl-zoom` 62 then passed.
Rust: `m2-editor` 260, `m2-server` 220, `m2`, `music2-desktop`'s menu tests; clippy of all four
clean. The full `ci/check.sh` is the End's.

**For the merge: the shared files lane B touched** (each in a small block of its own):

| File | What |
|---|---|
| `web/src/App.tsx` | `chord()`: =, +, −, ↑, ↓, ⇧F (and `page` for ⇧⌘=); `key()`: Z, ⌥←, ⌥→; `appCommand` cases for lane B's nine ids |
| `web/src/lib/shell.ts`, `web/src/lib/graph.ts` | `canvas.zoom`, `zoomFit`, `follow`, `height`, `marker`; `editorHooks.zoom` (lane D fills it) |
| `web/src/components/CodeView.tsx` | Two `MONACO_CHORDS` names, `shorter` and `taller` (X5) |
| `web/src/components/GraphCanvas.tsx` | `closeSong` sends waiting view settings first |
| `web/src/components/SongBar.tsx` | `RulerMenu` only: `onMarker` and its row |
| `web/src/components/Shortcuts.tsx` | Rows at the ends of Transport and View |
| `apps/music2-desktop/src/menu.rs` | Lane B's nine rows and their test |
| `crates/server/src/graph.rs`, `graph/actor.rs` | `Published.local`, the state's `local`; `mod markers`; the timeline's `markers` |
| `crates/server/src/lib.rs`, `routes.rs` | `mod routes_view`; its router merged |
| `crates/editor/src/lib.rs`, `layout.rs`, `timeline.rs` | `mod markers`; `layout_tx`, `commit`, `unreadable` made `pub(crate)`; `TimelineView.markers` |
| `apps/m2/src/edits.rs` | `times: None` in the duplicate gesture |
| `e2e/run.mjs`, `e2e/README.md` | `pl-zoom` and `pl-arrange` after `s8-automation` |
| `e2e/s8-songbar.mjs`, `e2e/s8-clips.mjs` | One check each names the new menu row |
| `e2e/lintlib.mjs` | The taste lint as a function (P0b-1's; lane A has the same) |

## Lane C

Songs and tracks.

## Lane D

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
