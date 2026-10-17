# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground.

## Lane A

Audio in the app.

## Lane B

Record what you play.

## Lane C

MIDI files and stems.

## Lane D

Plain-language help. Branch `studio-d`.

### D1. A tooltip on every control

**In short**

| What | Now |
|---|---|
| Hold the mouse over a button, knob, fader, tab or switch | A short line says what it does, and its key |
| A track's header | "Kick: click to show it in the dock below · drag to move it · right-click for more" |
| M and S | "Mute while you listen (M)", "Solo: hear only the soloed tracks (S)" |
| A knob in Device | How to use it comes first. Where it is in your code comes last |
| The key's Major and Minor | "Major: a bright, happy sound", "Minor: a darker, sadder sound" |
| A file in Code | What the file is: "song.ts: the song's main file. It brings the tracks together" |

**What changed**

- `e2e/tiplib.mjs` (new; P0 hadn't landed, so D made it, as the plan's §7.1 gives D the file after P0):
  `tips(sel, keys)` runs in the page. It asks for a tooltip on an icon button, a toggle (`aria-pressed`), a tab, a
  switch, a slider, a knob or fader (`[data-knob]`), a spin button, and every control `keys` names (selector → its
  key as `chord()` spells it, which the tooltip must hold). A text button that says its action and has no key needs
  none ("Done"), nor a segmented choice whose words name it ("Minor") (D19). Menu and list rows and the code
  editor's insides are left out (Q43). A tooltip is the control's own `title`, or an ancestor's that holds no other
  control; a knob's may sit round its slider. A tooltip that only repeats the control's words is listed too.
- Titles, attribute-only hunks (§7.2):
  - `SongTimeline.tsx`: the track header (`[data-module-tab]`, a `role=tab`) says what a click, a drag and a
    right-click do; the Song row says what it shows.
  - `NoteView.tsx`: Length's − and + ("One bar shorter", "One bar longer").
  - `CodeView.tsx`: each file's row says what the file is, in place of its path again.
  - `KeyField.tsx`: Major and Minor say how they sound.
  - Reworded (jargon or the wrong thing first): `MuteSolo.tsx` M ("Mute while you listen (M)") and S ("Solo: hear
    only the soloed tracks (S) · ⌥-click: solo only this track"); `NodeGraph.tsx` a Device knob: "Drag (⇧ for fine)
    · double-click to type a value · ⌥-click to reset · right-click to automate it", then "In your code:
    tracks/bass.ts, line 10 (written as 0.5)" (it started with `tracks/bass.ts:10`).
- `e2e/st-tips.mjs` (new, kind `fix`, 44 checks), its `run.mjs` row after `taste` and its README row.

**Tests.** `st-tips`: the lint's own check (planted controls: an icon with no tooltip, a toggle with none, a
tooltip that repeats "Export", a key not named; a "Done" button and a menu's row pass), then one check per place:
the start screen, New Song, the top bar, the track headers, M's and S's words, the timeline (as it opens, Bass's
automation shown, a clip picked, a marker), the Repeat popover, a marker's name field, the side browser, the dock's
header, a Device knob's words, the Device view of every track and the song, a knob's menu, the effect browser, Notes
and its tools, a note's menu, Mix, Code and its file list, Change track ID, Save As, Settings, the ⌥K sheet, + Track's
list, a toast, the Delete dialog, the key's popover, the mutes and solos popover, the export question, the status
popover with broken code, the keyboard piano's card, a MIDI keyboard's mark (injected), Rename Song (an app page's
menu), a PC page's top bar and dock header (Ctrl+Shift+E, Ctrl+Alt+B, Alt+1), and the taste lint. 44 of 44.
Moved (values only, no count changed): `pc-keys` 37, `taste` 39, `s8-mute` 152, `s5-knobs` 120, `code` 58,
`pl-notes` 102, `pl-mix` 47, `s8-mix` 80, `s8-tracks` 75, `pl-ids` 39, `s8-audition` 51, `nx-effects` 47, all ok.

**Red first** (`dist-before`, the server at `85b069a`): 35 of 44, 9 failed. 13 controls had no tooltip or one that
only repeated its words, on 5 kinds of place; and the words checks:

```
FAIL  the track headers: every control says what it does (5: no tooltip at <div[role=tab][data-module-tab=kick]> "Kick M S"; …)
FAIL  M and S say what they do: Mute (M) | Solo (S) · ⌥-click: solo only this track
FAIL  a knob's tooltip leads with how to use it, then where it is in the code: tracks/bass.ts:10  (written as 0.5) / Drag (⇧ for fine) · …
FAIL  Notes, its tools and keys: every control says what it does (2: no tooltip at <button> "One bar shorter"; no tooltip at <button> "One bar longer")
FAIL  the Code view and its file list: every control says what it does (1: a tooltip that repeats its words ("song.ts") at <button[data-file=song.ts]> "song.ts")
```

The first ten without one: the five track headers (Kick, Hats, Bass, Keys, Song), Length's − and +, song.ts's row
(its path again). Most of the app had its tooltips already (157 titles in `components/` at `85b069a`).

**Guards.** The lint's own first check (it can't be red on the old page: it tests `tips`, which is new).

**Left.** Lanes A, B and C's places join `st-tips` at the End (the Record button and its menu, the take, the
Export dialog, the tempo question, the guide, the volume popover, the drop hint), as the plan says. The read-only
window's banner and a hidden lane's "Show" line aren't walked (text buttons that say their action).

**For gaps.md:** #11: every control has a tooltip in plain words with its key, and `e2e/tiplib.mjs` checks it.

**Contract:** taste.md §5.3 gains: "Every icon button, toggle, tab, switch, slider, knob and fader has a tooltip in
plain words, with its key when it has one; a text button that says its action needs none. `e2e/tiplib.mjs`' `tips`
checks it, and every new suite runs it on what it adds."

## Merge

The lead joins the lanes.

## End

The lead's last checks, docs and pictures.
