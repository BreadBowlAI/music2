# Polish: build notes

What each part of the polish track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8c/plan.md`. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix and sounds.

## Lane B

Timeline.

## Lane C

Songs: recent songs, Save As, Rename Song, the delete question's keys, view settings kept.

There was no P0 when this lane started: lane C built the seams it needs itself, in its own files
where it could (`lib/modal.ts`, `DeleteDialog.tsx`), and in small blocks of the shared ones.

### C5. The delete question's keys

**In short.** The delete question now works like the export question. Return presses the button
with the focus ring. Tab moves the ring. No key gets through to the song behind it.

| Key | Before | Now |
|---|---|---|
| Return | always deleted | presses the focused button (Delete at first) |
| Space | played the song | presses a focused button; else nothing |
| Tab, Shift+Tab | left the dialog | go round its two buttons |
| M | muted a track behind it | nothing |
| Esc | cancelled | cancels |

**What changed**
- `web/src/lib/modal.ts` (new): `useModalKeys(root, { onEscape, onDefault, active })`, the keys of
  every modal dialog. It is `ExportAsk.tsx:47-66`'s logic, fixed for dialogs with fields (plan X14):
  Tab goes round every control (buttons, fields, boxes); a field keeps its own keys (Space types a
  space); Return in a field or a box, or with the focus on nothing, runs `onDefault`; Return on a
  button presses that button. No key reaches the page.
- `ExportAsk.tsx` uses it, with the same behaviour (`s8-mute`'s 152 checks pass).
- `web/src/components/DeleteDialog.tsx` (new): the delete question, moved out of `Tracks.tsx`
  (`joined`, `said`, `DeleteDialog`), on `useModalKeys`. Cancel got `data-testid=delete-cancel`.
  `Tracks.tsx` only exports `Asking` and `what` and imports the dialog: lane A's parts
  (`InstrumentPicker`, `TrackMenu`) are not touched.

**Tests.** New suite `pl-tracks` (kind `fix`, 20 checks), after `s8-tracks` in `run.mjs`: Delete has
the focus at first; Space and M with the focus on nothing do nothing behind it; Tab and Shift+Tab go
round the two buttons; Return on Cancel and Space on Cancel delete nothing and leave the start
point at beat 8; Esc cancels; Return on Delete deletes Kick and its Duck; ⌘Z puts every byte back;
the dialog passes the taste lint. Moved and green: `s8-tracks` 75, `s8-mute` 152, `taste` 39.

**Red first** (the page at `29430e8`):
```
FAIL  Space with the focus on nothing does nothing: nothing plays, nothing is deleted
FAIL  Tab and Shift+Tab go round the two buttons, never behind the question: BODY → browser-toggle → BODY → delete-confirm
FAIL  Return on Cancel deletes nothing: kick.ts is gone
```

**Left.** Nothing.

**For gaps.md.** The delete question's keys are fixed (8b-ui's loose end).
**Contract.** None.

### C1. View settings kept

**In short.** The grid and snap of the timeline and of Notes, and the audition switch, are now kept
with each song. Open the song again, even after quitting the app: they are as you left them. A new
song starts with the usual settings.

| Setting | Before | Now |
|---|---|---|
| Grid and snap (timeline, Notes) | this browser's storage: lost at each app launch | the song's `.music2/view.json` |
| Audition on or off | the same | the same file |

**What changed** (P0b-2's view store, built here as there was no P0)
- Server: `view_file::set(dir, changes)` merges a map into `.music2/view.json` (null removes an
  entry). It and the loop's `write` hold one lock, and each temporary file has its own name (X15).
  `Graph::set_view(session, set)` takes only `zoom`, `follow`, `heights`, `hidden`, `grid` and
  `audition`, refuses `loop` and any other name in words, and in a read-only window keeps them in
  memory only. The state carries them as `local` (read from the file when the song opens, without
  the loop) and `local_stamp` (moves when the server changes them itself; lane A's track ID change
  will use it, X20).
- Route: `POST /api/graph/view {session, set: {key: value | null}}` in `routes_project.rs` (not in
  `routes_view.rs`, which is lane B's). Not a write: no 423 in a read-only window.
- Page: `web/src/lib/local.ts` (new): `useLocal(key, fallback)`, `getLocal`, `setLocal(key, value)`
  and `flushLocal()`. Seeded from `graph.local` when the song or the stamp changes. Each change is
  sent at once; changes made while one is on its way go together after it. `pagehide` sends what
  waits with `keepalive`. `flushLocal` is for Save As and Rename (X37).
- `grid.ts` keeps both views' grids in `local.grid` (`{song: {pick, triplet, on}, notes: {…}}`);
  `audition.ts` keeps `local.audition` (`false` when off; on is the default, so it is removed).
  Neither reads `localStorage` now (no backwards compatibility).
- `e2e/s8-clips.mjs`: the two helpers that read the roll's grid from `localStorage` read the state's
  `local.grid.notes` now.

**Tests**
- Rust `crates/server/src/graph/view_file_tests.rs`: `view_settings_merge_and_keep_the_loop` (a
  set beside the loop, a null, the loop route keeping the settings, `loop` and an unknown name
  refused, another session refused, read again at the next open with unknown entries kept in the
  file, a read-only window writing nothing).
- New suite `pl-projects` (kind `none`, 15 checks so far), after `newsong`: it starts its own
  servers on a copy of first-song. Server 1: the roll's grid 1/8, the timeline's snap off, audition
  off, and `view.json` holds them. Server 2 (another port, another browser context): all three are
  as set. Another song opened in the same window starts with the defaults; back to the first, its
  settings come back.
- Moved and green: `s8-clips` 94, `s8-audition` 51, `notes` 52, `taste` 39, `newsong` 71.
- `cargo clippy -p m2-server --all-targets -D warnings` clean. `cargo test -p m2-server`: 218 of 219
  in the full run; `graph::tests::loading_is_published_while_a_plugin_is_created` failed once under
  the machine's load (other tracks building) and passed three times alone. It times a plugin's
  creation; nothing here touches it.

**Red first**
- Rust, with `set_view` a stub that keeps nothing:
  ```
  panicked at crates/server/src/graph/view_file_tests.rs:33:5: assertion `left == right` failed
    left: Object {}
   right: Object {"audition": Bool(false), "grid": Object {"notes": Object {"on": Bool(true), "pick": Number(0.125), "triplet": Bool(false)}}}
  ```
- `pl-projects` on the page and server at `29430e8` (with the old std):
  ```
  FAIL  .music2/view.json holds them: null
  FAIL  the roll's grid is 1/8 after a new launch: 1/16
  FAIL  audition is still off: true
  ```

**Left.** The comment above `s8-clips`' reload check still says "the app starts over"; it no longer
does (one line, lane B's part of that file). Zoom, follow, heights (lane B) and hidden lanes (lane D)
can use the same store: `useLocal("zoom", 24)` and `setLocal("zoom", 36)`.

**For gaps.md.** View settings are kept per song in `.music2/view.json` (8b-ui Q7 closed).
**Contract.** `docs/architecture.md` FMT-20 / the HOME table: `view.json` holds `loop` and the page's
settings `zoom`, `follow`, `heights`, `hidden`, `grid`, `audition`, written by
`POST /api/graph/view`.

## Lane D

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
