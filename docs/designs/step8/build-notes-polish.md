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

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
