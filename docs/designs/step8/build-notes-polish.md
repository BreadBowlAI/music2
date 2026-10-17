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

**Left.** A trackpad pinch is checked only as Chrome's Ctrl-wheel; the Mac's `gesture*` events
need the Mac run. Follow is a plain menu row, not a ticked one.

**For gaps.md.** Zoom (#9) and follow the playhead are done.

**Contract.** None beyond the view store's.

## Lane C

Songs and tracks.

## Lane D

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
