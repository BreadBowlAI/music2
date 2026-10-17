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

## Lane D

Notes and automation.

## Merge

The lead's merges of the four lanes.

## End

The lead's last check, docs and pictures.
