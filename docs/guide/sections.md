# Sections: copy a chorus

**In short**
- A **section** is a run of whole bars of the song, on every track at once: an intro, a verse, a chorus.
- Drag along the top of the ruler, where the markers are, to select bars. Then copy and paste them, duplicate them, empty them, or insert and delete bars.
- Each of these is one Undo step. Agents use `m2 sections`, or `POST /api/graph/sections`.

Pop songs are built from sections. Once a chorus sounds right, you copy it to where the next chorus goes, on every track at once: the drums, the bass, the chords, the automation and the audio.

## Copy a chorus

1. Drag along the top strip of the ruler (where the markers are) over the bars of the chorus, for example from bar 9 to the end of bar 16. Each end goes to the bar line nearest it. The bars show across every track, with a blue outline. A click on that strip selects one bar.
2. Press ⌘C (Ctrl+C on Linux) to copy them.
3. Click the ruler below that strip, at the bar where the next chorus goes (bar 25, say). The playhead goes there.
4. Press ⌘V (Ctrl+V). The chorus lands at the start of the playhead's bar, on every track.

What was in those bars before is replaced: each track now plays the chorus there. To make room instead, insert bars first (see below). Undo (⌘Z) takes the whole paste back.

A marker gives the same bars: right-click it, **Select to next marker**.

A drag across the rest of the ruler sets the loop, as it always has. It never selects bars.

## What you can do to selected bars

Right-click inside the selected bars for their menu. Each row is one Undo step.

| Do | Keys (Mac / Linux) | What happens |
|---|---|---|
| Copy | ⌘C / Ctrl+C | The bars go on the clipboard: what every track plays in them |
| Cut | ⌘X / Ctrl+X | Copy, then empty them |
| Paste | ⌘V / Ctrl+V | The copy goes at the playhead's bar, in place of what is there |
| Duplicate | ⌘D / Ctrl+D | A copy right after them, in place of what is there. The copy is then selected, so ⌘D again adds another |
| Empty bars | ⌫ / Backspace | What plays in them goes; the bars stay, empty |
| Insert bars | ⌘I / Ctrl+I | As many empty bars as are selected, where they start. Everything from there on moves later |
| Delete bars | ⇧⌘⌫ / Ctrl+Shift+Backspace | The bars go. Everything after them moves earlier to close the gap |
| Loop | ⌘L / Ctrl+L | Loop the selected bars |

One thing is selected at a time. Selecting bars lets go of the selected track and clips; picking a track, a clip or an automation point lets go of the bars. Esc lets go of them too, and so does a click on an empty spot in a lane.

## Good to know

- **Clips that cross the edge** of the bars are cut there. The part inside goes with the bars; the part outside stays where it is. Automation keeps its value at the edge on both sides, and a choice's or a switch's lane starts its part with what was playing at the edge.
- **A note belongs to the bar it starts in.** A note that starts before the bars you copy stays where it is, and isn't copied, even if it is still sounding when the bars begin. A note that crosses the edge is shortened to it.
- **The bars must fit where they go.** If the time signature is different there, so the bars would end partway through a bar, the paste is refused, and it says where they would end. Pick a place where they end at the end of a bar.
- **Insert bars and delete bars move the song's time too:** the markers, and the tempo and time signature changes, move with the music after them. New bars take the time signature and tempo of the bar before them. Automation that was moving through the place where you insert bars holds its value there over the new bars; notes and audio leave them empty.
- **When you delete bars,** the tempo and time signature in effect at their end stay, so the music after them sounds as it did, and so does the marker in effect there. If nothing comes after the bars you delete, their markers and changes go, and the song keeps its first tempo and time signature.
- **Copy, paste, duplicate and empty change only the clips.** The markers, tempo and time signature stay where they are.
- **If a data file doesn't read** (one you or an agent was editing by hand), these are refused, naming the file, until it is fixed: the bars can't move without it.
- These work while the code has an error too: they change only the data files, never the code.

## From the terminal

`m2 sections` does the same, on the bars you name: `9-16` (bars 9 to 16), `9` (one bar), or a marker's name (`Chorus`: from that marker to the next).

```bash
m2 sections ~/Music/music2/my-song duplicate 9-16 --to 25   # a copy of bars 9–16 at bar 25
m2 sections ~/Music/music2/my-song insert-bars 17-20        # 4 empty bars where bar 17 starts
m2 sections ~/Music/music2/my-song delete-bars Bridge       # the Bridge's bars go
```

With the app open, an agent can also copy bars with `POST /api/graph/sections/copy` and paste them with `m2 sections <song> json`. See [m2 commands](commands.md#m2-sections) and [API routes](routes.md).
