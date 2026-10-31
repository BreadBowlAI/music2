# Sections: copy a chorus

**In short**
- A **section** is a run of whole bars of the song, on every track at once: an intro, a verse, a chorus.
- Drag across the ruler to select bars. Then copy and paste them, duplicate them, empty them, or insert and delete bars.
- Each of these is one Undo step. Agents use `m2 sections`, or `POST /api/graph/sections`.

Pop songs are built from sections. Once a chorus sounds right, you copy it to where the next chorus goes, on every track at once: the drums, the bass, the chords, the automation and the audio.

## Copy a chorus

1. Drag across the ruler, over the bars of the chorus (for example from bar 9 to the end of bar 16). Each end goes to the bar line nearest it. The bars show in blue across every track, and they loop, so you hear the chorus.
2. Press ⌘C (Ctrl+C on Linux) to copy them.
3. Click the ruler at the bar where the next chorus goes (bar 25, say). The playhead goes there.
4. Press ⌘V (Ctrl+V). The chorus lands at the start of the playhead's bar, on every track.

What was in those bars before is replaced: each track now plays the chorus there. To make room instead, insert bars first (see below). Undo (⌘Z) takes the whole paste back.

A marker gives the same bars: right-click it, **Loop to next marker**. Its bars are selected too.

## What you can do to selected bars

Right-click inside the selected bars for their menu. Each row is one Undo step.

| Do | Keys (Mac / Linux) | What happens |
|---|---|---|
| Copy | ⌘C / Ctrl+C | The bars go on the clipboard: what every track plays in them |
| Cut | ⌘X / Ctrl+X | Copy, then empty them |
| Paste | ⌘V / Ctrl+V | The copy goes at the playhead's bar, in place of what is there |
| Duplicate | ⌘D / Ctrl+D | A copy right after them, in place of what is there. The copy is then selected, so ⌘D again adds another |
| Delete | ⌫ / Backspace | The bars stay, empty |
| Insert bars | ⌘I / Ctrl+I | As many empty bars as are selected, where they start. Everything from there on moves later |
| Delete bars | ⇧⌘⌫ / Ctrl+Shift+Backspace | The bars go. Everything after them moves up to close the gap |
| Loop | ⌘L / Ctrl+L | Loop the selected bars |

Esc lets go of the selected bars. So does a click on a clip or on an empty spot in a lane.

## Good to know

- **Clips that cross the edge** of the bars are cut there. The part inside goes with the bars; the part outside stays where it is. A note that crosses the edge is shortened to it, and automation keeps its value at the edge on both sides.
- **Insert bars and delete bars move the song's time too:** the markers, and the tempo and time signature changes, move with the music after them. New bars take the time signature and tempo of the bar before them. When you delete bars, the tempo and time signature in effect at their end stay, so the music after them sounds as it did, and so does the marker in effect there.
- **Copy, paste, duplicate and delete change only the clips.** The markers, tempo and time signature stay where they are.
- To select beats rather than bars, hold ⌘ (Ctrl) while you drag across the ruler. That only sets the loop.
- These work while the code has an error too: they change only the data files, never the code.

## From the terminal

`m2 sections` does the same, on the bars you name: `9-16` (bars 9 to 16), `9` (one bar), or a marker's name (`Chorus`: from that marker to the next).

```bash
m2 sections ~/Music/music2/my-song duplicate 9-16 --to 25   # a copy of bars 9–16 at bar 25
m2 sections ~/Music/music2/my-song insert-bars 17-20        # 4 empty bars where bar 17 starts
m2 sections ~/Music/music2/my-song delete-bars Bridge       # the Bridge's bars go
```

See [m2 commands](commands.md#m2-sections) and [API routes](routes.md).
