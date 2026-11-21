# The Playground: what was built

**In short**
- The Playground is a narrow list at the left of the piano roll, in the dock's Notes view. It is not a view of its own.
- It opens on one selected notes clip: ⌥P, or **Open Playground** in the clip's menu.
- Opening on selected bars (cutting the clip first) is not built. The rest of [the design](design.md) is.
- The rule is [VER-6](../../architecture.md#a-clips-versions); the suite is `e2e/playground.mjs`.

## Where the panel lives, and why

| Choice | Why |
|---|---|
| Inside Notes, at the roll's left, 224 px wide | The design says the roll shows the version that plays while you flip. A view of its own (a sixth tab) would hide the roll. A popover or a sheet would cover it |
| A list down the side, not a strip across the top | Names read best one to a row, and five to ten versions fit without scrolling. The roll gives up width, which it has, not height, which it needs for pitches |
| On the left | The list picks, the roll shows: the same order as the Code view's file list |
| Opening shows Notes and selects the clip | The Playground is about one clip, and Notes already shows the selected clip. So "the clip Notes shows" and "the Playground's clip" are one thing, and it closes when they stop being the same |
| No state in the song or its view file | It is attention, like the selection: the open clip and the loop to put back live in the page (`web/src/lib/playground.ts`), and the focus record says so to agents (AGENT-6) |

## Keys

| Key | Does | Why this one |
|---|---|---|
| ⌥P (Alt+P) | Open on the selected notes clip; close when open | P for Playground. ⌥ as the dock's other keys (⌥1 to ⌥5). Plain P is a note on the keyboard piano |
| ← → | Play the version before / after | The design's keys |
| S | Star the version that plays | The design's key |
| Esc | Close | The design's key |

While it is open the Playground takes ←, →, S and Esc before the rest of the window does (S is otherwise Solo; the arrows otherwise nudge a selected clip). It leaves them alone:

- while you type in a field or the code editor, and while a menu or a dialog is open;
- when the roll has the keyboard and notes are selected: the arrows move the notes, and Esc lets go of them;
- S, while the keyboard piano is on: it is a note.

A quick second → goes on from the version the first one asked for, not from what the song said a moment ago.

## What each part is

| Part | Where |
|---|---|
| The open clip, the remembered loop, every act | `web/src/lib/playground.ts` |
| The list | `web/src/components/Playground.tsx`, placed by `Dock.tsx` |
| The waiting versions' notes behind the roll's (faint, no clicks) | `NoteView.tsx` |
| The count mark on a clip, drawn the same on both paths | `SongTimeline.tsx` (the page's clips), `zoomPicture/clips.ts` (the picture drawn during a zoom); its size is `VERSIONS_MARK` in `ClipDrawing.tsx` |
| `playground` in the focus record | `web/src/lib/focus.ts`, `crates/server/src/graph/focus.rs` |
| `"play": true` on `add` (Duplicate in one undo step) | `crates/editor/src/versions.rs` |

## Changed from the design

| The design | Built | Why |
|---|---|---|
| With bars selected, the clip is first cut at their edges, one undo step | Not built. It opens on a whole clip | One undo step for two cuts needs a clip gesture that cuts at two places (Split cuts at one, and refuses a clip with waiting versions). That is a server change of its own. It is in [gaps.md](../../gaps.md) |
| "The roll above shows the one that plays" | The roll is beside the list, at its right | See "Where the panel lives" |
| Duplicate, then edit | Duplicate also plays the copy, in the same undo step | A copy that waits can't be edited in the roll: only the playing version is. So `add` takes `"play": true` (VER-3) |

## Left

- Opening on selected bars (above).
- No Open Playground row in the desktop app's menu bar: the clip's menu and ⌥P have it.
- Rename is in a row's ⋯ menu only. A double-click on a name was tried and dropped: the first click also plays the row.
- While it is open, ← and → flip versions and don't nudge the selected clip.
- A song with no loop set before: closing turns the loop off, at the clip's bars. The server has no way to unset a loop.
- A list longer than the panel scrolls, but doesn't scroll by itself to the row that plays.
- The mark on the timeline is tested on the page's clips. That the picture drawn during a zoom matches it to the pixel is checked by eye, not by a suite.
