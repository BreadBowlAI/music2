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

## Lane C

Songs: recent songs, Save As, Rename Song, the delete question's keys, view settings kept.

## Lane D

Notes and automation: the note menu and tools, the song's key, automation on its own scale, hiding
one automation lane.

There was no P0 when this lane started: lane D made the seams it needs itself, in its own files
where it could, and in small blocks of the shared ones (each named below).

### D1. A menu on notes, and note tools

**In short.** Right-click a note in Notes. A menu opens. It can move notes up or down, put them on
the grid, make them longer, loosen them up, or swing them. Each one is one undo step.

| Row | Key | What it does |
|---|---|---|
| Up / Down a semitone | ↑ / ↓ | moves the notes one key |
| Up / Down an octave | ⇧↑ / ⇧↓ | moves the notes 12 keys |
| Quantize to 1/16 | Q | puts each note's start on the grid |
| Legato | — | each note lasts until the next one starts |
| Humanize | — | moves timing (up to 10 ms) and loudness (up to 8%) a little |
| Swing: Light · Medium · Heavy | — | every second note a little later (55%, 60%, 66%) |
| Duplicate | ⌘D | copies the notes after them |
| Delete | ⌫ | takes the notes out |

- The menu acts on the selected notes. With none selected, it acts on every note: its first line
  says "All 12 notes". Then it shows no arrow keys, since those move only selected notes (X28).
- Right-click on a note that isn't selected selects it, alone. On empty grid the selection stays.
- Read only (the code doesn't compile): the menu opens, every row greyed, its tooltip saying why.
- Esc closes it. The roll keeps the keyboard all along.

**What changed**
- `web/src/lib/noteTools.ts` (new, pure): `transpose`, `quantize`, `legato`, `humanize`, `swing`.
  Each takes the notes and the picked indices and gives the notes after, or null when nothing
  changes. Swing **sets** each off-beat note to its swung place (X27): twice is the same as once,
  and Light after Heavy moves them back. A note is off-beat when it starts from a quarter step
  before its pair's middle to just past Heavy's place (1.35 steps in), so a change of amount finds
  them. A swung note keeps its end, so it never runs into the next note. Pairs are eighths when the
  roll's grid is 1/8 or wider, else sixteenths (Q26).
- `web/src/components/NoteMenu.tsx` (new): the menu, on `Popover` and `MenuRow` (taste.md §2.14),
  the Swing row a segmented control, the tooltips in X41's words.
- `web/src/components/NoteView.tsx`: the grid's right-click; Q (the selection, or every note);
  each tool one lane write with its own undo name ("transposing Bass notes", "quantizing Bass
  notes to 1/16", "making Bass notes legato", "humanizing Bass notes", "swinging Bass notes": the
  house's "Undo renaming the song" style, not the plan's "Transpose Bass notes"). The arrow keys
  ignore ⌥ (X1). A Ctrl-click on a Mac starts no gesture: it is a right-click there (X7). The
  grid's tooltip ends "right-click for note tools". Hooks: `editorHooks.quantize` (X2) and
  `editorHooks.zoom` (X3, the roll's own zoom ×1.5) while the roll has the keyboard.
- Shared blocks: `lib/graph.ts` declares the two hooks; `App.tsx`'s `quantize` asks
  `editorHooks.quantize` first (X2). `Shortcuts.tsx`: two rows at the end of Edit.
- `crates/editor/src/notes.rs` (outside the plan's list; no lane owns it): `describe` gave "4 notes
  → 4 notes" for any edit of as many notes either side. The page names an undo step only when the
  two sides differ, so every multi-note edit in the roll (a tool, a drag of several notes) had an
  unnamed Undo. Now it adds what tells them apart: their pitches ("4 notes, A1 to C2 → 4 notes, A2
  to C3"), where the first plays, their lengths or their velocities; else ", changed".

**Tests**
- New suite `pl-notes` (kind `fix`, 61 checks with D1), after `s8-audition`: see `e2e/README.md`.
- Rust: `notes::tests::an_edit_is_described_by_what_it_changed` gains five cases.
- Moved and green: `notes` 52, `fixes-notes` 37, `s8-audition` 51, `pc-keys` 37, `taste` 39,
  `s8-clips` 94, `s4-clips` 28, `g11-play` 24, `undo` 10. `m2-editor`: 251 tests and clippy.

**Red first** (the page and server at `29430e8`; then the page without X1 and X7; then the
server without the label change):
```
FAIL  right-click on a note opens its menu
FAIL  its rows, in order, with their keys: []
FAIL  ⌥→ and ⌥← with notes selected move no note: they are the markers' keys
FAIL  a Ctrl-drag on a note on a Mac moves nothing: Ctrl-click is a right-click there
FAIL  Undo names it: Undo (⌘Z)
  left: "4 notes → 4 notes"
 right: "4 notes, A1 to C2 → 4 notes, A2 to C3"
```

**Left.**
- `App.tsx`'s `zoom_in` and `zoom_out` (lane B's or the lead's) must ask `editorHooks.zoom?.(dir)`
  first, so ⌘= and ⌘− zoom the roll while it has the keyboard (X3). The hook is there.
- Edit ▸ Quantize through the menu bar is checked only in the app (no browser suite has the menu
  bar): the End's Linux run.

**For gaps.md.** Note tools are done: a right-click menu with transpose, quantize (Q), legato,
humanize and swing. Left: swing amounts other than three, groove templates.
**Contract.** None.

### D2. The song's key

**In short.** The top bar has a Key field after the time signature. Click it and pick a key, like
"A minor". Notes then shades the rows that are not in the key. Nothing is locked. The key is saved
in `layout.json`, so it goes with the song.

| What | How |
|---|---|
| Pick a key | Click "Key", choose Major or Minor, then a root (C, C♯, D, E♭, E, F, F♯, G, A♭, A, B♭, B) |
| Change Major / Minor | With a key set, Major or Minor changes it at once |
| Take it out | No key |
| Undo | ⌘Z: "Undo key A minor" |

**What changed**
- `crates/editor/src/songkey.rs` (new): `SongKey { root, scale }`, `read(&Layout)`, `of(dir)`,
  `set(s, Option<SongKey>)`. One Tx of `layout.json` alone (no revision, one undo step), as
  reorder is: `Expect::Identical`, based on the file on disk. Roots read by any name (`C#`, `C♯`,
  `Db`, `D♭`) and are written in ASCII (`C#`, `Eb`, …) so a hand or a script can write them; the
  label uses ♯ and ♭ ("E♭ major"). Scales: major and minor. The key is kept in `Layout.other`, so
  `layout.rs` didn't change: a new key goes right after `markers`, and an old one stays in its
  place. Refused while `layout.json` doesn't read (R15). Setting the key it has writes nothing.
- `TimelineView.key` (`timeline.rs`, its three places), published in the timeline's head
  (`graph/actor.rs`). The watcher already republishes when `layout.json` changes, so a key written
  by hand shows.
- `crates/server/src/graph/songkey.rs`: `Graph::song_key`. `crates/server/src/routes_notes.rs`
  (new router, merged in `routes.rs`): `POST /api/graph/song-key {session, key: {root, scale} |
  null}`, a write (423 read only). Named `song-key`, apart from a track's ID (X42).
- Web: `lib/songKey.ts` (the call, the undo name, the pitch classes in a key, `TimelineView.key`
  by `declare module`); `KeyField.tsx` (the field and its picker); `TopBar.tsx` mounts it in the
  tempo group and **measures** the name's room (Q23): twice the distance from the bar's middle to
  the nearer group, less 8 px a side, from the free space between the groups (a `ResizeObserver`).
  Under 56 px the name goes. `SongBar.tsx`: `SongTitle`'s field keeps to that room (160 to 480 px
  when there is room). `NoteView.tsx`: `Lanes` shades the pitch classes out of the key (the grid
  says which in `data-shaded`); a lane that names its drums (lane A's `names`) is never shaded by
  the key (X26).

**Tests**
- Rust `crates/editor/src/songkey_tests.rs`: `a_key_is_written_into_layout_json`,
  `no_key_removes_it`, `markers_and_unknown_keys_stay`, `a_scale_that_isnt_major_or_minor_is_refused`,
  `undo_puts_it_back`, `an_unreadable_layout_refuses`.
- `pl-notes` grows to 98 checks (see `e2e/README.md`). The test key is E minor (X32): F♯'s row
  plain, F's shaded, the other way round from the black keys.
- Moved and green: `s8-songbar` 82, `fx-fixes` 75, `taste` 39, `layout` 39, `notes` 52. `m2-editor`
  257 and `m2-server` 218 tests, clippy on both.

**Red first** (the tests against a stub `set` that refuses; the suite on D1's page):
```
test songkey::tests::a_key_is_written_into_layout_json ... FAILED
test songkey::tests::undo_puts_it_back ... FAILED
FAIL  the top bar has a Key field
FAIL  with no key, the roll shades the black keys' rows: F♯ (66) shaded, F (65) plain
```

**Left.**
- More scales, scale lock, chords (scale help, gaps #16).
- X26 is in the code but untested here: no lane names its drums until lane A lands.

**For gaps.md.** #6 (key and scale): the song's key is done, major and minor, in the top bar and
`layout.json`; Notes shades the rows out of it. Left: other scales and scale help (#16).
**Contract.** FMT-19: `layout.json` gains `key: {"root": "A", "scale": "minor"}` (ASCII roots),
written by `POST /api/graph/song-key`, not by `/api/graph/layout`. The timeline view gains `key`.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
