# music2 taste direction, written before step 8

Revision 2. This is the visual and interaction direction for music2's UI. A builder can apply it without
guessing. It follows the owner's brief: simple yet elegant, professional enough for a producer like
A. G. Cook, light grey and muted, no dark mode, every tool visible, no built-in AI, a native Tauri app.
Revision 2 folds in the problems raised against design-8b that reach the UI (§7): fewer toasts, fewer
markers, no header level slider, no status edge, native shortcuts only for ⌘ and ⌃ chords, and a dock
whose view the selection never changes.

It is based on:
- `.git/m2-step8/audit-ui.md`: 12 defects, the producer and designer critiques, and the top 20 problems;
- the 98 screenshots in `scratchpad/ux8/shots/` (cited below by their prefix, such as `b01` or `f05`);
- the web code at HEAD `cbf5eb9`;
- the contract's rules that reach the UI: HOME-1 (show every value's home), MON-1 to MON-7 (mute,
  solo, mute in song, latches, audition), FMT-19 (`layout.json`), WRITE-22a and RENDER-27a (leases).

Where this file and the audit disagree, this file wins. Where this file and `docs/architecture.md`
disagree, the contract wins, and this file should be fixed.

In one line: **Logic's clarity, Live's speed, and nothing on screen that isn't the music or a control
for it.**

---

## 0. How to apply it

Apply it in slices, so that each 8b feature is built on the new system and nothing is restyled twice:
T (tokens), then S (shell), then each 8b item with its own UI. §8 lists them.

**Checks for every slice:**

- Re-run the audit's shot scripts (`scratchpad/ux8/{a-start,b-first,…}.mjs`) at 1440×900. Look at
  every shot next to its old version.
- Add `e2e/taste.mjs`, a lint that walks the DOM of each shot state and fails on any of these:
  - a computed `font-size` outside {11, 12, 13, 15} px (skip anything inside `.monaco-editor`);
  - a `border-radius` outside {0, 2, 4, 8} px that isn't `rounded-full` (at least 9999 px);
  - a `background-image` that contains `gradient`;
  - a `backdrop-filter`;
  - `font-family` mono outside `[data-code]` elements.

  It also checks the font itself (§2.1): a Mac page draws its text in the system font and never
  loads Inter; a Linux page loads Inter and draws its text in it, with tabular figures.
- The e2e suites select by `data-testid`. Keep every testid on the element that replaces the old one.
  The status popover's items keep `attention` and `compile-banner`, and the pill keeps
  `read-only-banner`. Update the suites in the same PR that moves an element.

---

## 1. Principles

Each principle has one *do* and one *don't*, taken from this app.

### 1. The music is the loudest thing on screen
Strong colour belongs to clips, notes, meters and the selection. The chrome is grey.
- **Do:** give the selected clip the darkest outline in the window (2 px `fg-strong`), so selection is
  the strongest signal on any track colour.
- **Don't:** let the gradient "m2" tile and the gradient play button be the two most saturated objects
  on screen while the selected track is only "slightly whiter" (`f05`, `b01`).

### 2. Each thing appears once
Each track has one name (its label) and one row. There is one place for status, and one way to open
an editor: select the thing.
- **Do:** show read-only as one pill in the top bar, and grey out the controls that can't edit.
- **Don't:** say it three times, in a blue banner, a red Needs attention row and a toast (`d15`, `d16`).
  Don't add module tabs that repeat the track list and overflow at 16 tracks (`b01`, `f02`).

### 3. Speak music; keep the code one keystroke away
Use the words a producer uses and the descriptors' labels and units. Code text appears only where it
is literally code, and only then in mono.
- **Do:** the Undo button's tooltip says "Undo Hats volume". Name the automation lane "Decay". Call a
  module key the track's "ID": "key" means only the musical key.
- **Don't:** "song.ts: -6 → 12" (`c03`), a lane called `out-decay` (`c07`), `lowpass` in the
  browser (`c09`), or `bass.ts:11` under every node (`f07`).

### 4. Controls, not captions
Every pixel in a header, card or strip either shows the music or changes it.
- **Do:** make a track header hold a colour, a name, M, S and a meter.
- **Don't:** use that space for "track" and "48 notes" (`f06`), or show a colour legend and a
  "Drag a named output…" tutorial on every module, forever (`b01`).

### 5. Hands stay on the keyboard, and every drag is precise
Every command has a key that follows Logic or Live (§5). Every continuous control has fine drag,
type-in, reset and a readout at the cursor.
- **Do:** ⇧-drag for fine, double-click to type "-3" or "2k", ⌥-click to reset, and show the
  shortcut in every menu row.
- **Don't:** let a 40 px drag move −6 to +12 dB (`c02`), or spend ⌘E on Export when every Live user
  reaches for it to split.

### 6. Flat, quiet and light
One accent, no gradients, no glass or blur. Shadows go only on things that float. Use two radii for
controls and surfaces (2 px is only for small marks), four type sizes and hairlines rather than
cards.
- **Do:** separate regions with a 1 px `line` and a change of surface (chrome or canvas).
- **Don't:** put a rounded card inside a panel inside a window (`b02-…-kick`), or use seven radii
  and ten type sizes.

### 7. Show what you hear, and where each value lives
HOME-1 asks the UI to show every value's home. Keep "listening only" and "saved in the song" visibly
different. Never let the screen claim something that isn't true.
- **Do:** show a default (not written in code yet) in `fg-muted` text whose tooltip says so, and mark
  an automated value with a filled dot. Show "muted in song" as a struck-through name and hatched
  clips. Show "2 muted · 1 solo" in the top bar while any overlay is on.
- **Don't:** use grey italics for defaults (`b02-…-bass` "*4 dB*"). Don't say "No tracks yet" for
  a song with four tracks (`g02`). Don't use a pause glyph for what is really a stop.

### 8. Nothing moves unless the music does
The layout is stable: banners never push content down, popovers stay where they opened, toasts
don't pile up. Motion is short (220 ms at most) and only marks a change of state. The playhead and
meters move at audio rate through `requestAnimationFrame` and are never eased.
- **Do:** show problems in one fixed pill that takes no layout space.
- **Don't:** let the compile, Needs attention and read-only banners push the arrangement down 130 px
  (`d15`), let the effect browser jump as it filters (`c09` and `c11`), or ease the playhead with a
  200 ms transition between polls.

### Words and numbers

| The code says | The UI says | Never shown to a producer |
|---|---|---|
| module (track) | track, with its label ("Shakers & Hats") | the key, except in Details and in Code |
| module key (ID-1) | the track's ID ("Change track ID…", only in the Code view's file tree) | "key", which in the UI means only the musical key (C minor) |
| module `song` | Song (the bottom row); Master (the last mixer strip) | `song.ts:13` |
| lane (control) | automation lane, labelled by the control ("Cutoff") | `out-decay` |
| revision | (nothing) | "rev 7", except in the code view's status line |
| Tx, commit, base | (nothing) | always hidden |
| slot `channels.hats.db` | "Hats volume" | the slot path |
| overlay | "listening only" | "overlay" |
| latch (MON-6) | "overridden" | "latch" |
| diagnostic code `data.unclaimed` | a sentence, plus a Details disclosure that holds the code | the code in the main text |

- Write labels in sentence case with no final period. A menu item that opens a dialog ends in "…".
- Use a true minus (−, U+2212) in UI values, never in code.
- Formats:
  - dB: 1 decimal, "−6.0 dB";
  - Hz: whole numbers below 1 kHz ("350 Hz"), then 1 decimal ("1.2 kHz", "12.5 kHz");
  - ms: a whole number;
  - s: 2 decimals;
  - 0…1 values: 2 decimals;
  - pan: "C", "L 20" or "R 35" (percent);
  - semitones: "+7 st";
  - BPM: up to 2 decimals, with trailing zeros trimmed;
  - position: bar.beat.sixteenth ("5.3.2");
  - time: m:ss.t ("0:09.4").

---

## 2. Design system

### 2.1 Font and type scale

**Font: the system face on the Mac, Inter elsewhere.** On the Mac, `system-ui` resolves to SF Pro
in both Tauri's WebKit and headless Chrome. The menu bar and file dialogs are already SF, and a pro
tool doesn't need a typeface of its own: its identity is the work on screen. Off the Mac,
`system-ui` is some other face (DejaVu Sans on Linux, wide and plain), so the app ships one font
file: Inter 4.1's variable font, `web/public/fonts/InterVariable.woff2`, under the SIL Open Font
License (`OFL.txt` beside it). `main.tsx` sets `data-os="mac"` on the root on a Mac, before the
first render, and `index.css` picks the font by it, not by a font list (`system-ui` exists
everywhere). **Code uses `ui-monospace`** (SF Mono in WebKit, Menlo in Chrome). Nothing is fetched
from the network, which fixes D10: the app looks the same offline.

| | Mac | Linux, Windows |
|---|---|---|
| UI | `system-ui` (SF Pro) | Inter, from the app's own file |
| Code | `ui-monospace` | the system's monospace |

**Numbers are tabular everywhere.** Set `font-variant-numeric: tabular-nums` on `body` and never
opt out, so values don't jiggle as they change. Use no other `font-feature-settings`: today's
`"cv11", "ss01"` are Inter features.

| Token | Size / line | Weights | Used for |
|---|---|---|---|
| `text-xs` | 11 / 14 px | 400, 500 | units, lane values, ruler numbers, menu shortcuts, captions, M/S letters (600) |
| `text-sm` | 12 / 16 px | 400, 500, 600 | **the default UI size**: buttons, menu rows, tabs, control labels, lane names, card titles (600), strip names |
| `text-base` | 13 / 18 px | 400, 500, 600 | track names (500), inputs, dialog text, toasts, popover body, the song name (600), code (mono) |
| `text-lg` | 15 / 20 px | 500, 600 | the transport position (500), dialog titles (600) |

- There are no other sizes: no 9, 10, 10.5, 12.5, 14, 19 or 30 px.
- There is no bold (700), no all-caps and no letter-spacing.
- Italic is not used to carry meaning (see §2.10).
- **Mono only where the text is literally in a file:** the code editor, file paths, the code preview
  at the foot of the effect picker, and Details disclosures. Mark those elements `data-code` so the
  lint can check them. Lane names, node titles, the position, the BPM and "rev" are not code.

### 2.2 Spacing and sizes

- **Spacing:** use Tailwind's 4 px scale, steps 1, 1.5, 2, 3, 4, 6 and 8 (4, 6, 8, 12, 16, 24 and
  32 px). Nothing else.
- **Control heights:**
  - `ctl-xs` 20 px: M/S buttons, chips, the fold chevron's hit area;
  - `ctl-sm` 24 px: dock tools, segmented controls, pills;
  - `ctl` 28 px: top-bar buttons, menu rows, inputs, the Export button;
  - 32 px: dialog buttons and inputs.
- **Hit targets** are at least 20×20 px. A knob's whole cell (56×56 px) drags it, not just the 28 px
  circle.
- The layout sizes are in §3.2. They are tokens (`h-bar`, `w-side`, …) mirrored in
  `web/src/lib/layout.ts`, because `SongTimeline.tsx` and `NoteView.tsx` compute positions from them.

### 2.3 Radii, borders, elevation

| Token | px | Used for |
|---|---|---|
| `rounded-xs` | 2 | notes in the roll, meter segments, colour chips |
| `rounded-sm` | 4 | every control: buttons, inputs, M/S, pills, segmented items, clips, menu rows |
| `rounded-md` | 8 | every floating or carded surface: popovers, menus, dialogs, toasts, device cards |
| `rounded-full` | — | only circles: knobs, port dots, the meter's clip dot |

- **Borders** are 1 px hairlines:
  - `line` between regions and rows;
  - `line-strong` around controls (inputs, outline buttons, M/S off).
- Don't use `ring-1` together with a shadow on cards, and don't use `black/[0.06]` borders.
- **Elevation:**
  - `shadow-pop` on menus, popovers, dialogs and toasts only;
  - `shadow-chip` on the selected item of a segmented control;
  - `shadow-lift` on a clip, card or note while it is dragged.
  - Nothing that sits in the layout has a shadow.

### 2.4 The grey ramp

Cool and quiet, with all steps at the same hue: oklch(L, 0.002–0.011, 255). The contrast column is
measured against `chrome` (#edeff2).

| Step | Hex | Contrast | Role token | Used for |
|---|---|---|---|---|
| 0 | `#fcfdfe` | — | `raised` | popovers, dialogs, inputs, device cards, toasts, white piano keys |
| 50 | `#f5f7f9` | — | `canvas` | content: the arrangement grid, the dock body, the roll grid, the mix area, the code editor |
| 100 | `#edeff2` | — | `chrome` | the top bar, browser, track header column, ruler, dock header |
| 150 | `#e6e8eb` | — | `hover` | hover fills on chrome; black-key rows in the roll; segmented control tracks |
| 200 | `#dcdfe3` | 1.16 | `line`, `press` | hairlines between regions and rows; pressed fills; beat lines |
| 300 | `#cbced2` | 1.37 | `line-strong` | control outlines; bar lines; knob tracks; the fallback line on automation lanes |
| 400 | `#a5aaaf` | 2.0 | `fg-off` | disabled text and icons; placeholders |
| 500 | `#7c8186` | 3.4 | `fg-faint` | text you don't have to read: units, ruler numbers, menu shortcuts, port labels |
| 600 | `#65696f` | 4.8 | `fg-muted` | secondary text and inactive icons; meter fill |
| 700 | `#4c5056` | 7.0 | — | active icons; the M button when on; black piano keys |
| 800 | `#33373c` | 10.4 | `fg` | primary text |
| 900 | `#202327` | 13.7 | `fg-strong` | the playhead, the selection outline, the song name, dialog titles |
| 950 | `#141619` | 15.7 | — | reserved |

Anything you must read to act uses `fg-muted` or stronger (at least 4.5:1).

### 2.5 The one accent

The accent is a muted ink blue, oklch(0.50 0.12 260).
- **Why blue:** macOS and Logic users already read blue as "selected, focused, on".
- It is far from the danger red and the warning amber.
- The old brand mauve sat inside the track palette's rose, and was also used for warnings.
- **The mauve retires to the app icon in the Dock.** It is not used inside the window.

| Token | Hex | Used for |
|---|---|---|
| `accent` | `#3862a7` | primary buttons (white text, 6.1:1), focus rings, Solo on, Play while playing, knob arcs while dragged, the cable being dragged, loop edges |
| `accent-hover` | `#4974ba` | hover on primary buttons |
| `accent-strong` | `#294e8b` | pressed primary; accent text on soft fills (6.8:1 on `accent-soft`) |
| `accent-soft` | `#e0eafb` | toggles that are on (Loop, Snap, Audition, Ghost); the active menu row; the selected browser row; the selected track header |
| `accent-range` | `#c8daf8` | the loop brace in the ruler while loop is on; the code editor's text selection |
| `accent-wash` | `#eff4fc` | the selected track's lanes in the arrangement; the loop region over the lanes |

**Scarcity rule:** the accent marks only *focus, on and selected-in-chrome*. Knob value arcs, meters,
cables and clip selection don't use it at rest.

### 2.6 Status colours

There are only two, for problems. Success and information get no colour: quiet means fine.

| Token | Hex | Used for |
|---|---|---|
| `danger` | `#af3d36` | errors, clipping, "Reconnecting…", destructive menu rows |
| `danger-strong` | `#90302b` | danger text on `danger-soft` (6.8:1) |
| `danger-soft` | `#ffe9e6` | the error pill; the code problems strip |
| `warn` | `#b07a20` | warnings, the meter between −6 and 0 dBFS. Never put white text on it. |
| `warn-strong` | `#7e541b` | warning text on `warn-soft` (5.8:1) |
| `warn-soft` | `#fbefd6` | the warning pill; find matches in the code editor |

Read-only is not a problem colour. It is neutral (`hover` fill, `fg` text, a lock icon).

### 2.7 Track colours

There are twelve muted hues with equal lightness and chroma, so no track shouts. Each colour has
three tones, made by one formula:
- `bar` = oklch(0.72 0.085 h): the header's colour bar, clip borders and notes in the roll;
- `tint` = oklch(0.905 0.032 h): clip bodies;
- `ink` = oklch(0.46 0.075 h): marks drawn inside clips (notes, hits, automation lines) and note
  borders.

`ink` on `tint` is at least 5.2:1 for every hue. **The muting is in the palette. Don't mute again
at draw time.** Today's `mute()`, which mixes 50% toward grey, and the 22–42% alpha both go.

| Name | h | bar | tint | ink |
|---|---|---|---|---|
| rose | 10 | `#d48e98` | `#f4d7db` | `#7c464e` |
| coral | 38 | `#d4927e` | `#f4d9d1` | `#7c4839` |
| amber | 72 | `#c69c68` | `#edddc9` | `#725124` |
| olive | 100 | `#b1a666` | `#e4e1c9` | `#625922` |
| lime | 128 | `#95af75` | `#d9e4ce` | `#4c6031` |
| green | 152 | `#7cb48a` | `#d1e6d5` | `#356443` |
| teal | 185 | `#61b6ab` | `#c9e7e2` | `#14655d` |
| cyan | 215 | `#60b2c6` | `#c9e6ed` | `#146272` |
| blue | 248 | `#79aad8` | `#d0e2f4` | `#345b80` |
| indigo | 278 | `#98a0da` | `#dadef5` | `#4e5482` |
| violet | 305 | `#b197d0` | `#e4dbf1` | `#624d7a` |
| plum | 340 | `#c990b5` | `#efd8e6` | `#744765` |
| *graphite* | — | `#9a9fa6` | `#dbdee2` | `#51565c` |

Graphite is for the Song row and Master strip only, and is never given to a track.

**Rules (they fix D8):**
1. A track's colour is stored **by name** in `layout.json` (FMT-19, HOME-3), never by index and
   never in code.
2. **Default order:** rose, teal, amber, indigo, green, plum, olive, blue, coral, cyan, violet, lime.
   Each colour is far in hue from the two before it.
3. **A project with no stored colours** gets colours in track order (§3.4), using the default
   order. Nothing is written yet.
4. **The first layout write freezes every track's current colour** in the same write, so adding a
   track never recolours the others. The first layout write is an add, a reorder or a recolour.
5. **A new track** gets the first colour in the default order that no track uses. When all twelve
   are used, it gets the least-used colour, with ties going to the default order.
6. **Colour picker:** in the track header's menu, under Colour ▸, a 6×2 grid of 16 px `rounded-xs`
   swatches with a check on the current one.
7. The New Song dialog's template thumbnails use `bar` colours, not today's saturated `HUES`.

The palette lives in TypeScript (`web/src/lib/palette.ts`) because colours are applied through
`style` on clips and notes. CSS only needs the UI tokens.

```ts
export const TRACK_COLORS = {
  rose:   { bar: "#d48e98", tint: "#f4d7db", ink: "#7c464e" },
  coral:  { bar: "#d4927e", tint: "#f4d9d1", ink: "#7c4839" },
  amber:  { bar: "#c69c68", tint: "#edddc9", ink: "#725124" },
  olive:  { bar: "#b1a666", tint: "#e4e1c9", ink: "#625922" },
  lime:   { bar: "#95af75", tint: "#d9e4ce", ink: "#4c6031" },
  green:  { bar: "#7cb48a", tint: "#d1e6d5", ink: "#356443" },
  teal:   { bar: "#61b6ab", tint: "#c9e7e2", ink: "#14655d" },
  cyan:   { bar: "#60b2c6", tint: "#c9e6ed", ink: "#146272" },
  blue:   { bar: "#79aad8", tint: "#d0e2f4", ink: "#345b80" },
  indigo: { bar: "#98a0da", tint: "#dadef5", ink: "#4e5482" },
  violet: { bar: "#b197d0", tint: "#e4dbf1", ink: "#624d7a" },
  plum:   { bar: "#c990b5", tint: "#efd8e6", ink: "#744765" },
} as const;
export const GRAPHITE = { bar: "#9a9fa6", tint: "#dbdee2", ink: "#51565c" } as const;
export const DEFAULT_ORDER = ["rose", "teal", "amber", "indigo", "green", "plum", "olive", "blue", "coral", "cyan", "violet", "lime"] as const;
```

### 2.8 Drawn graphics

| Thing | Look |
|---|---|
| Arrangement grid | `canvas` background; bar lines 1 px `line-strong`; beat lines 1 px `line`; sixteenth lines only when there are at least 12 px between them, in `hover`. Track rows are separated by 1 px `line`. Track backgrounds don't alternate. |
| Clip | `tint` body, 1 px `bar` border, `rounded-sm`, marks in `ink`. No top colour strip and no gradient. Clips never show a note count. Clip names come later, as a 14 px strip at the top of the clip in `text-xs` `ink`. |
| Automation clip | The same body. The curve is a 1.5 px `ink` line with a 10% `ink` fill under it. Points are 6 px `ink` circles; a selected point is a 6 px `fg-strong` circle with a 2 px white ring. |
| Fallback value (automation lane, no clip) | A 1 px dashed `line-strong` line (4 on, 4 off), drawn only where there's no clip. It is not in the track colour. |
| Playhead | A 1 px `fg-strong` line across the lanes and an 8×6 px `fg-strong` triangle in the ruler. No glow. It is driven by `requestAnimationFrame` from the transport clock (the last status's beat plus elapsed time × tempo), never by a CSS transition. |
| Loop | A 6 px brace along the bottom of the ruler: `accent-range` with 1 px `accent` ends while the loop is on, `line-strong` while it is set but off. While the loop is on, the lanes inside it get `accent-wash`. This replaces today's "sun" yellow. |
| Song end | A small "End" flag in the ruler at the song's end (as in the roll), with the lanes after it in `chrome`. |
| Meters | Peak meters drawn on a canvas at 60 Hz. Fill is `fg-muted` below −6 dBFS, `warn` from −6 to 0 and `danger` at 0 dBFS or more. Peak hold is a 1 px `fg-strong` tick held for 1.5 s, falling at 12 dB/s. A clip dot (4 px, `danger`) stays on until clicked. The scale runs from −60 to +6 dBFS. A track meter is 3 px wide (two 1 px bars with a 1 px gap); a strip meter is 2×3 px; the master is 2×5 px. |
| Port shapes (Patch view) | Shape, not colour: audio is a circle, control a diamond, notes a rounded square, all 8 px. Filled means connected, hollow means not. This replaces `KIND_COLOR` (rose, blue, green). |
| Cables | Audio: 2 px `fg-muted`. Control: 1.5 px `fg-faint`. Notes: 1.5 px `fg-faint` dashed 4/3. The cable being dragged: 2 px `accent`. A selected cable: `fg-strong`. |
| Knob | A 28 px circle with a 270° track (2 px `line-strong`) and a value arc (2 px `fg`, from minimum, or from 12 o'clock if the control is bipolar). The pointer is a 1.5 px `fg-strong` line. While dragged, the arc is `accent` and a value bubble sits at the cursor (§2.14). |

### 2.9 States

| Element | Rest | Hover | Pressed | On / selected | Disabled |
|---|---|---|---|---|---|
| Ghost button (toolbar) | transparent; icon `fg-muted` | `hover` fill; icon `fg` | `press` fill | `accent-soft` fill; icon `accent-strong` | icon `fg-off`; no hover |
| Primary button | `accent` fill; white text | `accent-hover` | `accent-strong` | — | `press` fill; text `fg-off` |
| Secondary button | `raised` fill; 1 px `line-strong` border; text `fg` | border `fg-off` | `hover` fill | — | text `fg-off`; border `line` |
| Segmented control | the track is `hover`; items are text `fg-muted` | item text `fg` | — | item gets `raised` fill, `shadow-chip`, text `fg` | text `fg-off` |
| Menu or list row | transparent | `accent-soft` (keyboard-active rows look the same) | — | a 14 px check in `fg` at the left | text `fg-off` |
| Input | `raised` fill; 1 px `line-strong` border | border `fg-off` | — | focus: `accent` border plus the focus ring | `chrome` fill; text `fg-off` |
| Track header | `chrome` | `hover` | — | `accent-soft` fill, the name in 600 `fg-strong`, and a 2 px `accent` line on its right edge where it meets the lanes; its lanes get `accent-wash` | (read-only: no hover) |
| Clip | `tint` and `bar` border | border `ink` | while dragged: `shadow-lift`, 90% opacity | 2 px `fg-strong` outline; body becomes `bar` at 35% over `tint` | a muted track's clips draw at 40% with `grayscale(0.6)` |
| Note (roll) | fill mixes `bar` toward `ink` by velocity; 1 px `ink` border | 2 px `ink` border | — | `ink` fill, white name, 1 px white inner ring and 1 px `fg-strong` outer | on a read-only lane: `line-strong` fill, no hover |
| Knob | §2.8 | track `fg-off`; `ns-resize` cursor | the value bubble shows | while dragged: arc `accent` | track `line`, value `fg-off`, can't drag; the reason is in its menu (`d06`, keep) |
| M button | 1 px `line-strong` border; "M" `fg-muted` | `hover` | `press` | **muted (listening only)**: `gray-700` fill, white "M" | "M" `fg-off` |
| S button | the same as M | | | **soloed**: `accent` fill, white "S" | the same as M |

**Mute states in full** (MON-2, MON-3, MON-5):

| State | M button | Track name | Clips | Home |
|---|---|---|---|---|
| Muted, listening only | `gray-700` fill | `fg-muted` | 40% and grey | local |
| Muted by someone else's solo | unchanged | `fg-faint` | 40% and grey | local |
| **Muted in song** (saved in code, exported) | `gray-700` fill (the strikethrough and hatching already say "in the song", so no extra dot) | struck through, `fg-muted` | 40%, grey, with 45° hatching in `line` | code |

Clicking M on a track that is muted in song doesn't toggle anything. It opens the header menu
with "Unmute in song" first, because that change is a Tx. The M key and drags across M buttons skip
it. Mute wins over solo, a mute in song included: S on it lights but plays nothing, and its tooltip
says "Muted in the song: unmute it to hear it".

A strip shared by several tracks ("Kick + Snare") shows a dash in M or S when its tracks differ.

**Error on a field:** the border turns `danger`, and an 11 px `danger` message appears below the
field. Nothing shakes.

### 2.10 Home markers (HOME-1)

A value whose home isn't code shows it with a 5 px marker just right of the value. Hovering the
marker says the home in words. Code has no marker, written or inherited: most values in a new song
are inherited defaults, and a dot after each of them would make the common case the noisy one. So
each marker means something. The markers and the `fg-muted` default text replace today's grey italics.

| Home | Marker | Value text | Tooltip |
|---|---|---|---|
| code, written | none | `fg` | — (the common case is quiet) |
| code, inherited (left out, at its default) | none | `fg-muted` | on the value: "Default. Not written in your code yet: changing it writes it in." |
| lane (automated) | filled `accent` circle | `fg`, moving with playback | "Follows the Cutoff lane." A drag on the knob shows the same words in its value bubble and changes nothing until latches exist; the knob's menu has Show Cutoff lane and Remove automation (KN-2). |
| latched (MON-6, overridden) | hollow `accent` circle | `fg-strong` | "Overridden by hand. Re-enable to follow the lane again." (Re-enable and Write to code are in its menu) |
| cable | the knob is replaced by "← wobble" in `fg-muted` | — | "Driven by wobble." |
| state (plugin parameter, later) | filled 4 px square, `fg-faint` | `fg` | "Saved in the plugin's state." |
| local (mute, solo, zoom) | no marker; the M and S states above say it | — | — |
| layout (order, colour) | no marker | — | — |

### 2.11 Focus

```css
:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
:focus:not(:focus-visible) { outline: none; }
```

- One ring everywhere. It replaces the browser's blue on the Graph tab (`e09`) and the mauve
  elsewhere.
- Inside canvases (clips, notes, cards), keyboard focus *is* the selection, so there is no extra
  ring.
- Chrome buttons don't take focus on a mouse click (`onMouseDown: preventDefault`). That way Space
  stays the transport after a click. Tab still reaches every control.

### 2.12 Icons

- **One set: Lucide** (`lucide-react` 1.55.0, already a dependency).
- Always `strokeWidth={1.5}` with `absoluteStrokeWidth`, in `currentColor`.
- Sizes: **16 px** in the top bar and dock header; **14 px** in rows, menus, chips and cards;
  **12 px** only for fold chevrons.
- Transport glyphs are filled (`fill="currentColor"`): `SkipBack`, `Play`, `Square` (stop) and,
  later, `Circle` (record).
- **One icon per meaning, shown once:**
  - loop `Repeat`;
  - undo `Undo2`, redo `Redo2`;
  - export `Download`;
  - status `TriangleAlert`, `CircleAlert`, `Lock`, `WifiOff`;
  - add `Plus`;
  - more `Ellipsis`;
  - search `Search`;
  - audition `Volume2`;
  - draw `Pencil`, select `MousePointer2`;
  - ghost notes `Layers`;
  - snap `Magnet`;
  - zoom `ZoomIn` and `ZoomOut`;
  - metronome `Metronome`;
  - dock views: Device `SlidersHorizontal`, Notes `Piano`, Mix `SlidersVertical`, Code `CodeXml`;
  - fold `ChevronRight`.
- All of these are in the installed version, so there are no custom icons.
- A category's icon appears once, in its browser header, never on each item.
- No emoji. No icon tiles (the 28 px tinted squares in the knob menu and examples menu go).

### 2.13 Motion

| Name | ms | Easing | Used for |
|---|---|---|---|
| fast | 80 | linear | hover and press colours |
| base | 140 | `--ease-out` | menus and popovers opening (opacity, plus rising 4 px); toggles; segmented controls |
| slow | 220 | `--ease-out` | dialogs opening (opacity, plus scale from 0.98); the dock showing or hiding |
| exit | 100 | `--ease-in` | anything leaving (opacity only) |
| lit | 1200 | `--ease-out` | a lane or track just made (today's `lit` flash: keep it, with `accent` at 18% and a 3 px `accent` inset edge) |
| none | 0 | — | the playhead, meters, knob, fader and cable drags, zoom, scroll |

- No `active:scale`, no `hover:brightness`, no `transition-all`.
- Under `prefers-reduced-motion`, every duration becomes 0, so `lit` is skipped. The new lane is
  still selected and scrolled into view.

### 2.14 Components

- **Menus and popovers:** one style for all of them, replacing today's three.
  - Surface: `raised`, `rounded-md`, `shadow-pop`, 4 px padding.
  - Rows: 28 px tall, `rounded-sm`, `text-sm`, an optional 14 px icon on the left, the shortcut
    right-aligned in `text-xs` `fg-faint`.
  - An optional detail line in `text-xs` `fg-muted`, used only where the detail decides the choice,
    such as an effect's description.
  - Separators: 1 px `line` with 4 px above and below. Width 220–320 px.
  - Placement: anchored to the trigger, flipping only at the window edge. The position is fixed when
    it opens and doesn't move while a list filters (fixes `c09` and `c11`).
- **Toasts** (fix D7):
  - **only for a result that is off screen or structural:** adding, duplicating or deleting a track,
    mute in song, a track ID change, automating a knob or removing its automation, export, refusals.
    A knob, fader, pan, tempo or meter change, a clip move or resize, a note edit, a reorder or a
    recolour shows none: its result is on screen, and a mixing pass is hundreds of fader releases.
    Its words name the undo step instead (the Undo tooltip, Edit ▸ Undo "Hats volume");
  - one at a time, centred over the arrangement, 16 px above its bottom edge: above the dock while it
    is open, so a toast never covers the Mix strips' faders or the roll;
  - `raised`, `rounded-md`, `shadow-pop`, `text-base`, at most 440 px wide, one line;
  - the action (Undo) in `accent` at 500;
  - 4 s, or 6 s with an action; hovering pauses the timer; a new toast replaces the old one.
  - The text is words from the descriptor ("Added Reverb after Keys"), never a diff.
- **Pills** (status, overlays): 24 px, `rounded-sm`, `text-sm` at 500, a 14 px icon, padding
  8 px. Clicking one opens a 360 px popover. A pill has no × of its own: whatever it clears is in the
  popover.
- **Dialogs:**
  - `raised`, `rounded-md`, `shadow-pop`, 24 px padding;
  - the title in `text-lg` 600;
  - the footer is right-aligned, primary last;
  - Enter confirms and Esc cancels;
  - the backdrop is `fg-strong` at 20%, with no blur.
- **Value bubble:** shown while a knob or fader is dragged. It is `fg-strong` at 92% with white
  `text-xs` tabular text, `rounded-sm`, and sits 12 px above the cursor ("−3.4 dB").
- **Type-in:** a double-click on a value swaps it for an input of the same size with the text
  selected. Enter commits, Esc cancels and Tab moves to the next control. It accepts "-3", "-3db",
  "2k", "2.5 khz", "120ms", "0.5s", "50%", "L20", "+7" (semitones) and "C4" (pitch).

### 2.15 The `@theme` block (paste-ready)

This goes in `web/src/index.css`. It replaces the current `@theme` block and the `.glass`,
`.pop-gradient` and `.pop-text` classes.

```css
@import "tailwindcss";

@theme {
  /* Only these tokens exist: Tailwind's defaults are cleared first. */
  --color-*: initial;
  --font-*: initial;
  --font-weight-*: initial;
  --text-*: initial;
  --radius-*: initial;
  --shadow-*: initial;
  --ease-*: initial;
  --animate-*: initial;

  --font-sans: "Inter Variable", sans-serif;  /* the Mac's own font under :root[data-os="mac"], §2.1 */
  --font-mono: ui-monospace, "SF Mono", Menlo, monospace;
  --font-weight-normal: 400;
  --font-weight-medium: 500;
  --font-weight-semibold: 600;

  --text-xs: 11px;   --text-xs--line-height: 14px;
  --text-sm: 12px;   --text-sm--line-height: 16px;
  --text-base: 13px; --text-base--line-height: 18px;
  --text-lg: 15px;   --text-lg--line-height: 20px;

  /* Layout and control sizes; mirrored in web/src/lib/layout.ts. The 4px scale is Tailwind's default. */
  --spacing-bar: 44px;      /* top bar */
  --spacing-ruler: 28px;
  --spacing-row: 44px;      /* track row, medium */
  --spacing-lane: 32px;     /* automation lane */
  --spacing-dockbar: 32px;  /* dock header */
  --spacing-side: 240px;    /* browser */
  --spacing-heads: 240px;   /* track headers */
  --spacing-ctl: 28px;
  --spacing-ctl-sm: 24px;
  --spacing-ctl-xs: 20px;

  --radius-xs: 2px;
  --radius-sm: 4px;
  --radius-md: 8px;

  --color-white: #ffffff;
  --color-gray-0: #fcfdfe;
  --color-gray-50: #f5f7f9;
  --color-gray-100: #edeff2;
  --color-gray-150: #e6e8eb;
  --color-gray-200: #dcdfe3;
  --color-gray-300: #cbced2;
  --color-gray-400: #a5aaaf;
  --color-gray-500: #7c8186;
  --color-gray-600: #65696f;
  --color-gray-700: #4c5056;
  --color-gray-800: #33373c;
  --color-gray-900: #202327;
  --color-gray-950: #141619;

  /* Roles. Components use these; the ramp is for drawn graphics. */
  --color-raised: var(--color-gray-0);
  --color-canvas: var(--color-gray-50);
  --color-chrome: var(--color-gray-100);
  --color-hover: var(--color-gray-150);
  --color-press: var(--color-gray-200);
  --color-line: var(--color-gray-200);
  --color-line-strong: var(--color-gray-300);
  --color-fg-off: var(--color-gray-400);
  --color-fg-faint: var(--color-gray-500);
  --color-fg-muted: var(--color-gray-600);
  --color-fg: var(--color-gray-800);
  --color-fg-strong: var(--color-gray-900);

  --color-accent: #3862a7;
  --color-accent-hover: #4974ba;
  --color-accent-strong: #294e8b;
  --color-accent-soft: #e0eafb;
  --color-accent-range: #c8daf8;
  --color-accent-wash: #eff4fc;

  --color-danger: #af3d36;
  --color-danger-strong: #90302b;
  --color-danger-soft: #ffe9e6;
  --color-warn: #b07a20;
  --color-warn-strong: #7e541b;
  --color-warn-soft: #fbefd6;

  --shadow-pop: 0 0 0 1px rgb(20 22 25 / 0.07), 0 10px 28px -8px rgb(20 22 25 / 0.2);
  --shadow-chip: 0 0 0 1px rgb(20 22 25 / 0.08), 0 1px 1px rgb(20 22 25 / 0.05);
  --shadow-lift: 0 4px 12px -4px rgb(20 22 25 / 0.18);

  --ease-out: cubic-bezier(0.2, 0, 0, 1);
  --ease-in: cubic-bezier(0.4, 0, 1, 1);
  --animate-rise: rise 140ms var(--ease-out) both;
  --animate-lit: lit 1200ms var(--ease-out) both;
}

html { color-scheme: light; }
body {
  @apply bg-chrome text-fg font-sans text-sm antialiased select-none overflow-hidden;
  font-variant-numeric: tabular-nums;
}
::selection { background: var(--color-accent-range); }
* { scrollbar-width: thin; scrollbar-color: var(--color-gray-300) transparent; }
:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
:focus:not(:focus-visible) { outline: none; }
@keyframes rise { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
@keyframes lit { 0%, 35% { opacity: 1; } 100% { opacity: 0; } }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { animation-duration: 0s !important; transition-duration: 0s !important; } }
```

Keep these rules that already follow the block in `index.css`, with these changes:
- `.lit` and `.lit::after`: use `--color-accent` instead of `--color-pop-500`, and
  `animation: var(--animate-lit)`;
- `.rise`: becomes the `animate-rise` utility;
- `html.native .cursor-pointer { cursor: default; }`: unchanged;
- the Monaco popup rule: `border-radius: 8px` and `box-shadow: var(--shadow-pop)`.

**From the old names to the new**, for the token slice:

| Old | New |
|---|---|
| `ink-950`, `ink-900` | `chrome` (the dock body and arrangement become `canvas`) |
| `ink-850` | `canvas` |
| `ink-800` | `raised` |
| `ink-700` | `hover` as a fill, `line` as a border |
| `ink-600` | `press` as a fill, `line-strong` as a border |
| `ink-500` | `fg-off` |
| `ink-400` | `fg-faint` |
| `ink-300` | `fg-muted` |
| `ink-200` | `fg` for text, `gray-700` for icons |
| `ink-100` | `fg` (headings and the selection outline use `fg-strong`) |
| `pop-400`, `pop-500`, `pop-600` | `accent-hover`, `accent`, `accent-strong` |
| `violet-glow` | — |
| `sun` | `fg-strong` for the playhead, `accent` and `accent-range` for the loop |
| `black/[0.06]`, `black/[0.08]` borders | `line` |
| `ring-black/10` | `line-strong` |
| `red-*`, `amber-*` | the `danger` and `warn` roles |
| `sky-*` (read-only) | neutral (`hover` fill, `Lock` icon) |
| Monaco `#8f6574…` | `accent` for the cursor; `accent-range` for the selection; `accent-wash` for word highlight; `warn-soft` for find matches |

---

## 3. The main window

### 3.1 Wireframe (1440×900, about 12 px per character)

```
┌────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ o o o  |< > Loop  5.3.2  0:09.4  124 BPM  4/4       My Song       ! 1 issue  2M 1S    DSP 23%  ▮▮  ↶ ↷ Export      │
├──────────────────┬────────────────────────┬────────────────────────────────────────────────────────────────────────┤
│ Search...        │ + Track            v>  │ 1       2       3  ┌──── loop ────┐5       6       7       8  End      │
│                  ├────────────────────────┼────────────────────────────────────────────────────────────────────────┤
│ Instruments      │ ▌   Kick         M S ▮ │ │▪ ▪ ▪ ▪ ▪ ▪ ▪ ▪│▪ ▪ ▪ ▪ ▪ ▪ ▪ ▪│▪ ▪ ▪ ▪ ▪ ▪ ▪ ▪│▪ ▪ ▪ ▪ ▪ ▪ ▪ ▪│      │
│   Bass           │ ▌   Clap         M S ▮ │         │  ▪   ▪   ▪   ▪   │  ▪   ▪   ▪   ▪   │                        │
│   Kick           │ ▌ v Bass         M S ▮ │ │- -- - --- -- -│- -- - --- -- -│- -- - --- -- -│- -- - --- -- -│      │
│   Pad            │       Cutoff 1.2 kHz * │ │/──────────────\│ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ │
│   Pluck          │       Drop       0.00  │ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ │/──────────────\│ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄ ┄   │
│ Effects          │ ▌   Hats         M S ▮ │ │' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' ' '│      │
│ v Filter         │ ▌   Hook         M S ▮ │                                  │= =  = = =  = =  = = =  = = =│       │
│   Low-pass       │  ...                   │                                                                        │
│   High-pass      ├────────────────────────┼────────────────────────────────────────────────────────────────────────┤
│ > EQ             │ ▌   Song             ▮ │ (the Song's own lanes: mix automation)                                 │
│ > Dynamics       ├────────────────────────┴────────────────────────────────────────────────────────────────────────┤
│ > Delay & Reverb │ ▌Bass  [ Device | Notes | Mix | Code ]                     [ Chain | Patch ]    ...   v         │
│ > Modulation     ├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ > Utility        │  ┏━━ Bass ━━━━━━━━━━━━━━┓ ┌─ Drive ──────┐ ┌─ Reverb ───────────────┐  [+]                      │
│                  │  ┃  (o)     (o)     (o)  ┃ │  (o)    (o)  │ │  (o)     (o)     (o)   │                          │
│                  │  ┃ Cutoff   Res    Drive ┃ │ Amount  Tone │ │ Size    Damp     Mix   │                          │
│                  │  ┃ <-blend  0.70  4 dB   ┃ │ 6.0 dB  0.50 │ │ 0.60    0.50    0.25   │                          │
│                  │  ┗━━━━━━━━━━━━━━━━━━━━━━━┛ └──────────────┘ └────────────────────────┘                          │
│                  │                                                                                                 │
└──────────────────┴─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Key:**
- `▌` is the track's colour bar, `M S` its mute and solo, and `▮` its meter. The level is the Mix
  strip's fader (§3.4).
- `v`/`>` are fold chevrons (Bass is unfolded and shows its two automation lanes).
- In the lane headers, `*` is the lane marker. Drop's `0.00` is an inherited default, drawn in
  `fg-muted` with no marker (§2.10), as is the Bass card's `4 dB`.
- `┄` is the fallback value, drawn only between clips.
- The Bass card's heavy top edge is its 3 px colour bar.

**The rules behind it:**
- **The browser runs the full height on the left,** as in Live. The dock spans only the area right
  of it, so you can drag from the browser into the dock or the timeline.
- **The Song row is pinned to the bottom** of the track list (Live's Main track). It holds the root
  module's lanes and the master meter. It has no M or S, because MON-1 says the root can't be muted
  or soloed.
- **The dock's content follows the selection; its view doesn't** (§3.7). There are no module tabs.
- **Status lives in the top bar's one pill.** Nothing else repeats it: no edge line, no banner.

### 3.2 Sizes

| Region | Size | Range | Notes |
|---|---|---|---|
| Window | 1440×900 reference | at least 1100×700 | Below 1280 px wide, the browser starts hidden. |
| Top bar | 44 px tall | fixed | The left 80 px are kept for the traffic lights (native only; 12 px in browser mode). Today it is 64 px with an 88 px inset. |
| Browser | 240 px wide | 200–360, or hidden (⌥⌘B) | Shown by default. Its width is per window (local). |
| Track headers | 240 px wide | 200–320 | Today 236. |
| Ruler | 28 px | fixed | Today 34. |
| Track row | 44 px (medium) | small 28, medium 44, large 72 (⌘↑ and ⌘↓) | The track's primary lane is the row itself (§3.4). Today notes lanes are 64 px. |
| Automation lane | 32 px | fixed | Today 48. Folded by default. |
| Audio lane | the track row's height | | Today 52. |
| Arrangement zoom | 24 px per beat by default | 2–192 px per beat; ⌘= and ⌘− step ×1.5; pinch | Ruler numbers thin to every 2, 4 or 8 bars when bars are less than 40 px wide. |
| Dock | 32 px header plus a 300 px body | the body is 180 px up to the window height minus 200; a double-click on the splitter hides or shows it (E) | At 1440×900 this leaves 496 px of lanes: 11 rows at 44 px, against 6 today (`f02`). |
| Splitters | 1 px drawn, 6 px hit area | | `row-resize` and `col-resize` cursors |

### 3.3 The top bar, left to right (1440 px)

| x (px) | Element | Spec |
|---|---|---|
| 0–80 | Traffic lights | `data-tauri-drag-region` across every empty part of the bar |
| 80 | Go to start | 28 px ghost, `SkipBack`. Return. |
| 112 | Play / Stop | 28 px. Stopped: ghost with a `Play` glyph in `gray-700`. Playing: `accent` fill with a white `Square`. It stops (and Space stops); there is no pause glyph. |
| 144 | Loop | 28 px ghost toggle, `Repeat`; `accent-soft` while on. C. |
| (176) | Metronome | 28 px ghost toggle (K), `Metronome`. It appears when the metronome exists (gaps #19), not before; everything to its right then moves 32 px right. |
| 188 | Divider | 1 px `line`, 20 px tall, with 12 px on each side |
| 200 | Position | "5.3.2" in `text-lg` 500 `fg`, 72 px wide. "0:09.4" in `text-sm` `fg-muted`, 52 px wide. Both tabular, neither mono. |
| 336 | Divider | |
| 348 | Tempo | A field 76 px wide: "124" in `text-base` 500 plus "BPM" in `text-xs` `fg-faint`. Drag up and down: 1 BPM per 4 px, or 0.1 with ⇧. Double-click to type. No ⌥-click reset and no tap tempo (§5.2). While a lease is live (RENDER-27a) the number is `accent`. Lands with 8b's tempo bar. |
| 428 | Meter (time signature) | A field 40 px wide, "4/4". Click opens a picker (beats 1–16, unit 2/4/8/16). Lands with the tempo bar. |
| (472) | Key | It appears when key and scale have a home (gaps #6 and #16; the contract has none yet). |
| centre | Song name | `text-base` 600 `fg-strong`, centred in the window. At most `100% − 2×480 px` wide, cut with an ellipsis. Its tooltip has the folder and sample rate (keep). |
| right − 520 | Status pill | One pill, and only when something needs saying (§4 ST-1). |
| | Overlay pill | "2 muted · 1 solo", while any monitoring overlay or latch is on. `accent-soft` fill, `accent-strong` text. A click opens its popover: each muted or soloed track with its own ×, and Clear all at the foot. The pill has no × of its own, since overlays aren't undoable and one stray click mustn't wipe a mute and solo set. Lands with 8b's mixer. |
| right − 300 | DSP | "DSP 23%" in `text-xs` plus a 24×4 px bar. It shows the highest callback load in the last second. It turns `warn` at 70% or more (gate 8's budget) and shows a `danger` dot for 3 s after a dropout. Clicking opens Audio settings (gaps #31). Lands with 8a. |
| right − 228 | Master meter | 64×12 px stereo peak bars plus a clip dot. Lands with 8b's meters (design-8b §10, the `out` tap). |
| right − 156 | Divider | |
| right − 144 | Undo, Redo | 28 px ghosts, `Undo2` and `Redo2`. Their tooltips name the change ("Undo Hats volume"). |
| right − 76 | Export | A ghost text button, "Export", with `Download` at 14 px; ⇧⌘E. Keep the progress ring that replaces the label while it renders (`d13`). |
| right − 12 | — | 12 px padding. The close ✕ is gone (⌘W). |

### 3.4 Track headers and lanes

**Track order:**
1. `layout.json` order (FMT-19), if it is there;
2. otherwise the mixer's channel order (found by following edges, ID-18);
3. otherwise the import order in `song.ts`.

Never alphabetical (fixes D8). Drag a header to reorder; the drop is a layout write (presentation,
no revision).

**A track's rows.** The **primary lane** is the first notes or audio lane in code order, and the
track row draws its clips. Any other notes lanes are full-height sub-rows that are always shown.
Control lanes are 32 px sub-rows, folded by default. A unfolds the selected track, or all tracks if
none is selected. "Automate" unfolds the track and flashes the new lane (`lit`).

**Header, 240×44 px, from x = 0:**

| x | w | Element |
|---|---|---|
| 0 | 4 | Colour bar (`bar`, full height, including the track's sub-rows) |
| 12 | 12 | Fold chevron (only if the track has control lanes) |
| 28 | 140 | Name in `text-base` 500 `fg`, cut with an ellipsis ("Shakers & Hats" and "Lead Vox Double" fit). Double-click (or ⌘R) to rename inline, in a field the width of the header. |
| 180 | 20 | M (`ctl-xs`, `text-xs` 600) |
| 202 | 20 | S |
| 231 | 3 | Meter, 28 px tall (two 1 px bars) |

The header has no "track" caption, no key, no lane kind, no note count and **no level slider**. A
44 px slider over −60 to +12 dB would move 1.6 dB a pixel (against principle 5), repeat the Mix fader
(principle 2), and cut the name to 95 px. The level is the Mix strip's fader, ⌥3 away.

**Lane header, 240×32 px:**
- the colour bar at 40%;
- the label at x = 28 in `text-sm` `fg-muted`: the control's label, "Cutoff" (`descriptor.label`,
  else the key in sentence case);
- the value at the playhead, right-aligned to x = 222, in `text-xs` `fg-faint`;
- the home marker at x = 228.

**Corner (240×28, above the headers, next to the ruler):**
- "+ Track": a ghost `ctl-sm` button with `Plus` at 14 px; ⌘T;
- on the right, a fold-all chevron (A).

This replaces "TRACKS" and "4/4 · 124 BPM".

**Header menu** (right-click, or ⋯ on hover):
- Rename ⌘R
- Duplicate ⌘D
- Delete ⌘⌫
- —
- Colour ▸
- Mute in song
- Show automation A
- —
- Edit in code (with `tracks/bass.ts` as the detail)

### 3.5 The dock

**Header (32 px, `chrome`):**
- At the left: the selection chip, which is an 8 px colour chip plus the name in `text-sm` 600.
  Clicking it opens the header menu.
- Then the segmented control `Device · Notes · Mix · Code` (⌥1 to ⌥4).
- Then the current view's tools, right-aligned.
- At the far right, `ChevronDown` hides the dock (E).
- The body is `canvas` and edge to edge: no card, no title row, no legend, no hint.

**Device** (the view at first launch; after that the dock keeps whichever view was chosen, §3.7):
- The track's sound as a chain, left to right, top-left aligned, at 100% scale, scrolling
  horizontally: the instrument card, then its effect cards, then a 28 px "+" slot. Today's Chords
  chain (`e02-…-chords`) is the model.
- **Card:**
  - `raised`, 1 px `line`, `rounded-md`, 12 px padding, at least 128 px wide;
  - the instrument card has a 3 px colour bar along its top;
  - header (24 px): the label in `text-sm` 600 ("Reverb", from `descriptor.label`) and ⋯ on hover;
  - the menu holds Bypass (later, gaps #21), Move left and right, Delete ⌫, —, Edit in code (with
    the file:line as the detail).
- **Controls** sit in 56 px columns: the knob (28 px), the label (`text-xs` `fg-muted`) and the
  value (`text-xs` `fg`, with its home marker).
- **When the sound isn't a chain:** if the audio path from the track's lanes to `out` isn't a line
  of components, Device opens Patch instead, with one `text-sm` `fg-muted` line in the header: "This
  sound is built from parts. Showing its patch." The Clap (`e02-…-clap`) is an example.
  - Modulators that feed controls, such as `lfo → xrange → blend` in the Bass, are not cards. The
    control they drive reads "← blend", and Patch shows them.
- **Song selected:** Device shows the song's own chain (duck, master). The mixer is one compact card,
  "Mixer · 16 channels · open Mix ⌥3", never a 16-row node (`f03`).
- **Tools:** `Chain · Patch` segmented.

**Patch** (sound design; today's graph, cleaned up):
- port shapes and neutral cables (§2.8);
- node titles in `text-sm` 600 from the label, with the code name in the tooltip;
- no file:line;
- Fit is clamped to 75–100%, top-left aligned;
- the zoom bar shows only when the content overflows.
- **Cable drag (fixes D3):** only compatible inputs get an `accent` ring. Others dim to 40% and say
  why on hover: "Frequency (Hz) can't drive Drive (dB)", using the label and never the slot (`amount`).

**Notes:**

```
│ ▌Bass / Notes · bars 1–8 │ [↖ | ✎] │ Grid [1/16 v] [3] [Magnet] │ Length [− 8 +] bars │        [Layers] [Volume2] [− +] │
```

- Header tools: Select or Draw (B); grid ▾ (1/4 to 1/64); triplet (⌘3); snap (⌘4); the clip
  length; ghost notes; audition (on by default); vertical zoom.
- Gone from the header: "48 notes", "New notes 1/16" (new notes take the grid's length, as in
  Live) and the ✕ (Esc clears the selection).
- **Keyboard (56 px):**
  - white keys are `raised` with `line` separators; black keys are `gray-700`, 60% of the width;
  - every C is labelled ("C3") in `text-xs` `fg-faint`;
  - the key for the hovered row is `hover`, and a sounding key is `bar`;
  - keys are playable once audition lands.
- **Grid:** white-key rows are `canvas` and black-key rows `hover`. Rows are 14 px by default
  (10–24 with ⌘↑ and ⌘↓). Outside the clip, 45° hatching in `line` (keep today's END hatching).
- **Velocity lane:** 64 px with lollipops (keep).
- **Click behaviour (fixes D4):**
  - a click on empty grid selects nothing and moves the insert point;
  - a double-click adds a note;
  - dragging on empty grid draws a marquee;
  - in Draw mode (B), a click adds a note and a drag paints.
- **Empty states:**
  - a selected track with no clip selected: "Select a clip, or double-click in the timeline to make
    one."
  - a selected clip with no notes: "Click in the grid to add a note" (keep, `c22`), changed to
    "Double-click…" to match the new behaviour.
  - Tabs are never disabled.

**Mix** (new with 8b's mixer):

```
┌──────┐┌──────┐┌──────┐       ┌────────┐
│▀▀▀▀▀▀││▀▀▀▀▀▀││▀▀▀▀▀▀│       │▀▀▀▀▀▀▀▀│   3 px colour bar (graphite for Master)
│Kick  ││Clap  ││Bass  │  ...  │Master  │   20  name, text-sm 500
│ (o)  ││ (o)  ││ (o)  │       │        │   32  pan knob, 24 px
│  C   ││ L 20 ││  C   │       │        │   14  pan value, text-xs
│[M][S]││[M][S]││[M][S]│       │        │   24  M and S, 24×20 each
│ ┃ ▮▮ ││ ┃ ▮▮ ││ ┃ ▮▮ │       │ ┃ ▮▮▮▮ │       fader: 4 px track, 20×10 thumb;
│ ┃ ▮▮ ││ ╋ ▮▮ ││ ┃ ▮▮ │       │ ╋ ▮▮▮▮ │       travel = body height − 136;
│ ╋ ▮▮ ││ ┃ ▮▮ ││ ╋ ▮▮ │       │ ┃ ▮▮▮▮ │       0 dB at 75% of the travel
│ ┃ ▮▮ ││ ┃ ▮▮ ││ ┃ ▮▮ │       │ ┃ ▮▮▮▮ │
│−6.0  ││ 0.0  ││−3.0  │       │−1.0 ceil   20  dB field, text-xs, double-click to type
│ducks…││      ││      │       │peak −0.8   16  sidechain row (only when it applies)
└──────┘└──────┘└──────┘       └────────┘
  76 px strips with 1 px line between them; Master is 96 px, pinned right after a 12 px gap
```

- Strips are in track order and named by label. Each channel is found by following edges (ID-18).
- **A channel that doesn't write `db`** (the Kick in first-song, `b02-…-song`) still gets a fader.
  It sits at 0 dB, its value in `fg-muted` (§2.10), and its first move writes `db` in (option 1 in
  understand-8b §3.2).
- **The foot of every fader is −∞.** The last 4 px of travel write `db: -96`, which std makes
  silence, and the dB field reads "−∞"; −60 dB sits just above the foot. ⌥-click resets to 0 dB.
- **A channel's first pan** is heard while dragging: the pan is written into the song as the drag
  passes 4 px, and the drag goes on from there (design-8b §5.3).
- **A strip shared by several tracks** ("Kick + Snare") has M and S that act on all of them (§2.9).
- **MON-2, "the mixer shows this":** when a muted track still feeds a sidechain or control input,
  the strip dims except for one `text-xs` `fg` row, "Still drives Duck on Bass": the component's
  label and the track it sits on, so the app never needs std's vocabulary. The words are enough;
  there is no pulsing dot.
- **Master:** no M or S (MON-1). It gets a fader once std's `master` takes a gain; until then, the
  ceiling knob and the peak readout.
- The strip menu (right-click) has Mute in song, Colour ▸, Rename and Edit in code.

**Code:**
- The file tree is 200 px wide (filenames are literal, so mono `text-sm` is fine), with the track
  colour dots (keep).
- Monaco at `text-base` in `ui-monospace`.
- A 24 px status line: "Compiles · playing rev 2". This is the only place a revision number appears.
- Code opens on the selected track's file.
- The file tree's menu on a track's file has **Change track ID…** (8b), the one place the UI offers
  a track's ID, which is literal code here.
- The changed-on-disk notice and the problems strip stay inside the code view, styled
  `warn-soft` and `danger-soft`.

### 3.6 The side browser

- **Search field:** 28 px, at the top, with `Search` at 14 px. Typing filters live, and ↓ moves
  into the list.
- **Sections:** Instruments and Effects; later Plugins (gaps #32) and Sounds (gaps #39).
  - Effects are grouped by `editor.category`: Filter, EQ, Dynamics, Distortion, Delay & Reverb,
    Modulation, Utility.
  - Each category header is 24 px in `text-sm` 600, with its icon at 14 px in `fg-faint` and a fold
    chevron. Items have no icons.
- **Rows:** 24 px, `text-sm`, using the descriptor's label ("Low-pass", "Reverb", "Duck"). The
  description and code name are in the tooltip.
- **Search** matches the label, the description and `editor.keywords`, a std addition: Duck gets
  `["sidechain", "pump"]`. Today searching "sidechain" finds nothing (`c12`).
- **Drag:**
  - onto a track header or the Device chain: add after the last effect (or at the drop slot);
  - onto empty arrangement space below the tracks: add a track with that instrument (8b add track);
  - Enter on a selected row adds it to the selected track.
- **The "+" in Device and the "+ Track" button** open the same list as a popover anchored to the
  button, with typeahead (today's behaviour, `c11`). One list in two places is the one allowed
  duplicate, because the popover is the keyboard path.

### 3.7 Selection drives the dock's content, never its view

The dock's view (Device, Notes, Mix or Code) is one choice for the window, kept in `view.json`. It
changes only when the producer changes it: ⌥1 to ⌥4, the segmented control, X, or a double-click on
a notes clip, which opens Notes on it. Selecting a track, a clip or the Song row never changes it, so
clicking Kick's header to press M or ⌘D while mixing in ⌥3 keeps the mixer in view. Device is the
view at first launch.

| You select | The dock shows (in the current view) | Notes |
|---|---|---|
| a track header | Device: its chain. Notes: its selected clip, or the empty state. Mix: every strip, its own highlighted. Code: its file | ⌥1 to ⌥4 switch views at any time |
| a notes clip (one click) | the clip is selected; the view stays | A double-click opens Notes on that clip. |
| an automation clip or lane | its track, as above | Automation is edited in place in the timeline (8b clip editing). The dock never shows a dead end (D11). |
| the Song row | Device: the song's chain. Mix: every strip. Code: `song.ts` | |
| nothing (Esc) | the last track's content | The dock never goes blank. |

### 3.8 Small windows

- At 1100 px wide the browser starts hidden and the headers shrink to 200 px.
- The top bar drops items in this order: the time readout, the DSP percentage text (the bar stays),
  then the song name.
- In the Mac app the bar starts 80 px in, for the window's buttons, and the browser's toggle sits
  before Go to start, so the left side ends about 500 px in once the time shows. There the time
  shows from 1280 px (not 1180), and from 1280 px the name keeps 548 px clear on each side (the
  480 plus the 68 px the window's buttons add). `fx-fixes` checks both bars at 1100, 1180, 1280
  and 1440 px (the taste plan's attack, item C).
- What the engine is busy with ("Paused for export", "Loading Surge XT…") is a neutral state of
  the status pill, ranked below every problem (ST-1): nothing sits under the song's name (lead
  decision T2).
- The dock body is never shorter than 180 px, and the lanes are never shorter than 3 rows.
- Nothing scrolls horizontally except the arrangement, the roll, the Device chain and the Mix
  strips.

---

## 4. Change list, screen by screen

Screens are ranked by how much time a producer spends in them, and changes are ranked within each
screen. Each change is marked **KEEP**, **CHANGE** or **REMOVE**. A **CHANGE** that adds something
new says "(new)".

**"Lands with"** says what delivers it:
- `T` and `S` are the token and shell slices (§8);
- `8b:mixer`, `8b:tracks`, `8b:clips`, `8b:audition`, `8b:tempo`, `8b:automate` and `8b:meters` are
  step 8b's items;
- `8a` is step 8a;
- `UI` is web only, any time after S;
- `later #N` is a gaps.md item outside step 8.

### TL: Timeline and track headers (`b01`, `e01`, `f02`, `f04`, `f06`, `g02`–`g06`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| TL-1 | CHANGE | Headers hold controls | Use the §3.4 layout: colour, fold, a 140 px name, M, S, meter, 44 px rows. Drop "track", the key, the lane kind and the note counts. No level slider: the level is the Mix fader. | `f06`, `c17` | S (layout); 8b:mixer (M, S) |
| TL-2 | CHANGE | Order and colour from `layout.json` | Order as in §3.4, colours by name (§2.7), drag to reorder, Colour ▸ in the menu. Freeze all colours on the first layout write. | `e01`, `f02` | 8b:tracks |
| TL-3 | CHANGE (new) | Add a track | "+ Track" in the corner and ⌘T open the instrument list, on the last instrument added. The new track lands below the selected one, selected and lit, with no rename open, so Space, M and S work at once (⌘R renames). The Empty template gets a centred primary "Add a track" button and the line "or write one in code ⌥4" in `fg-muted`. | `e11` | 8b:tracks |
| TL-4 | CHANGE | Selection is unmistakable | The header gets `accent-soft`, a 600 name and a 2 px `accent` right edge; its lanes get `accent-wash`. A selected clip gets a 2 px `fg-strong` outline with its body at `bar` 35%. | `b07`, `b01` | S |
| TL-5 | CHANGE (new) | Clip editing | Resize from either edge (6 px hit areas); ⌥-drag copies; ⌘D duplicates right after, on the next bar for a span of a bar or more (a marquee keeps its own bars); ⌘E splits at the playhead and ⌘J joins; drag up or down to another track's lane of the same kind; ⌘C and ⌘V at the playhead; snap follows the zoom (bar, beat, 1/16), and holding ⌘ bypasses it. No toast: the clips show it. | `g05`, `g06` | 8b:clips |
| TL-6 | CHANGE (new) | Draw automation in place | On the curve (within 6 px), a click adds a point and a drag moves it; elsewhere on the clip, a click selects the clip and a drag moves it, as for notes clips; ⌫ deletes. Draw mode (B) paints anywhere. Bending a segment (⌥-drag) waits for a curve field in the lane format. While playing, this is a lane lease on control lanes too (WRITE-22a). | `g03`, `g04` (D11) | 8b:clips |
| TL-7 | CHANGE | Automation lanes fold under their track | 32 px sub-rows, folded by default (A), labelled by the control's label, left-aligned, sans, with the value and home marker on the right. | `b01`, `c07` | S |
| TL-8 | CHANGE | The lane key and label made by Automate (D6) | The key is the control's name (`decay`), with the device's label in front only on a clash (`pluck-decay`). The label is "Decay". The toast: "Automating Kick decay in a new lane". | `c07`, `c08` | 8b:automate |
| TL-9 | CHANGE | Playhead | 1 px `fg-strong`, moved with `requestAnimationFrame` from the transport clock. No 200 ms transition, no yellow, no glow. | `b03` | S |
| TL-10 | CHANGE | Loop | The ruler brace and lane wash in §2.8, in place of the yellow. Drag in the ruler to set it (keep the gesture); ⌘L loops the selection. | `b04`–`b06` | T |
| TL-11 | CHANGE (new) | Zoom and track height | ⌘= and ⌘−, pinch, and Z (zoom to the selection, or to the whole song); heights 28/44/72 with ⌘↑ and ⌘↓; ruler numbers thin out when crowded. | `f02`, `f04` | UI (gaps #17) |
| TL-12 | CHANGE (new) | Show where the song ends | An "End" flag in the ruler, with `chrome` after it. A double-click past the end still makes a clip. | `g06`, `c22` | UI |
| TL-13 | CHANGE | Broken on open (D2) | Show the tracks and clips from `data/` and the tempo from `arrangement.json`, greyed, with one line at the top of the lanes: "Waiting for the code to compile." Never "No tracks yet". | `g02` | S (+ the server sends the last-known timeline) |
| TL-14 | CHANGE | Fallback line | 1 px dashed `line-strong`, drawn only between clips. | `b01` | T |
| TL-15 | CHANGE | Clip look | `tint` body, `bar` border, `ink` marks, `rounded-sm`. No top strip, gradient or double muting. | `f06` | T |
| TL-16 | CHANGE | Revealing a lane doesn't hide the first name (D12) | Set `scroll-padding-top` to the ruler's height; reveal with `block: "nearest"`. | `c07`, `d09` | S |
| TL-17 | REMOVE | "TRACKS" and "4/4 · 124 BPM" in the corner | The corner holds "+ Track" and fold-all; the tempo and meter move to the top bar. | `f06` | S |
| TL-18 | KEEP | Double-click empty lane space to make a 1-bar clip; the `lit` flash; clips addressed by id | | `c22`, `c07` | — |

### TB: The top bar (`b01`, `b03`, `f05`, `d13`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| TB-1 | CHANGE | 44 px tall, in the order of §3.3 | Today it is 64 px. | `f05` | S |
| TB-2 | REMOVE | The "m2" logo tile | The window title and the Dock already say which app this is. | `f05` | T |
| TB-3 | CHANGE | Play reads as on while playing | Stopped: a ghost `Play`. Playing: `accent` fill with a white `Square`. No gradient, no pause glyph, no dark-grey "disabled" look. | `b03` | T |
| TB-4 | CHANGE | Position | "5.3.2" (with sixteenths) in `text-lg` 500 sans, tabular, plus the time in `fg-muted`. Not mono. | `f05` | T |
| TB-5 | CHANGE (new) | Tempo and meter fields | As in §3.3; the lease shows in `accent`; the field is also in the New Song dialog. | `b01` | 8b:tempo |
| TB-6 | CHANGE (new) | Status pill | One slot for every problem and state (§4 ST-1). Replaces the banners. | `d05`, `d09`, `d15`, `f10` | S |
| TB-7 | CHANGE (new) | Overlay pill | "2 muted · 1 solo", with Clear all in its popover and no × on the pill. Required because MON-4 keeps overlays across reopening. | — | 8b:mixer |
| TB-8 | CHANGE (new) | DSP load | As in §3.3; puts gate 8's 70% budget in the app; a dropout dot. | — | 8a |
| TB-9 | CHANGE | Export | A ghost text button with ⇧⌘E (`menu.rs` too). Keep the progress ring. | `d13` | T |
| TB-10 | REMOVE | "rev N" | Moves to the code view's status line. | `f05` | T |
| TB-11 | REMOVE | Close ✕ | File → Close Song ⌘W (a new `close_song` item in `menu.rs`; `close_window` stays in Window). | `f05` | S |
| TB-12 | KEEP | Undo and Redo buttons | Their tooltips name the change once editor messages are words (gaps #29). | `f05` | S |
| TB-13 | CHANGE (new) | Master meter | 64 px stereo peak with a clip dot. | — | 8b:meters |
| TB-14 | KEEP | The song name centred, with the folder and rate in its tooltip | Restyled `text-base` 600. | `f05` | T |

### DK: The dock (`b01`, `b02-*`, `c18`, `f02`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| DK-1 | REMOVE | Module tabs (Song, Bass, Hats…) | The selection drives the dock (§3.7). Fixes D9's overflow. | `b01`, `f02` | S |
| DK-2 | CHANGE (new) | Dock header | Selection chip, `Device · Notes · Mix · Code`, view tools, hide. 32 px. | — | S |
| DK-3 | REMOVE | The outer rounded card, the "Bass track" title row, the legend and the hint | The canvas is the panel. | `b02-…-kick`, `e13` | S |
| DK-4 | CHANGE | No disabled tabs | Notes with no clip shows an empty-state line (§3.5). | `b01` | S |
| DK-5 | CHANGE | Splitter | 1 px, a 6 px hit area, a double-click hides or shows, height kept per window. | — | S |

### MX: Mix (new; `b02-…-song`, `e02-…-song`, `f03`, `f08`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| MX-1 | CHANGE (new) | Channel strips | As in §3.5: pan, M, S, fader, dB field, meter (once taps exist), name, colour. | `f03` | 8b:mixer |
| MX-2 | CHANGE (new) | Faders for channels with no `db` | An inherited 0 dB fader whose first move writes `db`, in the style the mixer object already uses (`db(-3)` beside `db(-6)`). Every fader's foot is −∞. | `b02-…-song` | 8b:mixer |
| MX-3 | CHANGE (new) | MON-2 made visible | "Still drives Duck on Bass" stays bright on a muted Kick's strip: words only, no pulsing dot. | — | 8b:mixer |
| MX-4 | CHANGE (new) | Mute in song | From the strip or header menu. It looks as in §2.9 and is a Confined Tx on the track's options. | — | 8b:mixer |
| MX-5 | CHANGE (new) | Master strip | Graphite, pinned right, no M or S; a fader once std's `master` has a gain. | `f08` | 8b:mixer + std |
| MX-6 | REMOVE | Mixing in the Song graph | The mixer node with `t01.db` rows at 50% zoom becomes one compact card (§3.5). | `f03` | 8b:mixer |

### DV: Device and Patch (`b02-*`, `e02-*`, `c13`, `c14`, `c21`, `d01`, `d02`, `f07`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| DV-1 | CHANGE (new) | The Device chain view is the default | As in §3.5, including the rule for when Device falls back to Patch. | `e02-…-chords` (the model), `e02-…-clap` | UI |
| DV-2 | CHANGE | Titles are labels | "Reverb", not `reverb`, in sans 600; the code name is in the tooltip. | `f07` | T |
| DV-3 | REMOVE | file:line under every title | It moves to the card menu's "Edit in code" detail. | `f07` | T |
| DV-4 | CHANGE | Home markers, not italics | As in §2.10. | `b02-…-bass` | UI |
| DV-5 | CHANGE | Patch view cleanup | Port shapes, neutral cables, Fit clamped to 75–100%, top-left, the zoom bar only on overflow. | `e02-…-clap`, `b02-…-kick` | T (shapes, colours); UI (fit) |
| DV-6 | CHANGE | Cable drag offers only what it accepts (D3) | Compatible inputs get an `accent` ring; others dim with a reason that uses labels. | `d01`, `d02` | UI |
| DV-7 | KEEP | "+" after a node opens the typeahead; effects can be deleted | Delete moves into the card menu and ⌫ on a selected card. | `c09`, `c14` | — |

### NT: Notes, the piano roll (`b08`–`b12`, `e03`–`e06`, `c22`, `e15`, `f09`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| NT-1 | CHANGE | A click doesn't add a note (D4) | The click rules in §3.5, plus Draw mode (B). | `b08` → `b09` | S |
| NT-2 | CHANGE (new) | Audition | Sound on note-down, while dragging across pitches, and on clicked keys. A header toggle, on by default. ⌘. is panic. | `e05` | 8b:audition |
| NT-3 | CHANGE | The keyboard is an instrument | As in §3.5: playable, every C labelled. | `f09` | UI + 8b:audition |
| NT-4 | CHANGE | Header | The §3.5 tools. Drop "48 notes", "New notes 1/16" and the ✕. | `f09` | S |
| NT-5 | CHANGE (new) | Vertical zoom and drum folding | ⌘↑ and ⌘↓ set rows from 10 to 24 px. "Fold" shows only used rows, which is the default for `hits` lanes. | `b11`, `e15`, `e03` (the top note is cut) | UI |
| NT-6 | CHANGE (new) | Ghost notes | Other tracks' notes in this bar range as 1 px `fg-off` outlines; a header toggle, off by default. | — | UI |
| NT-7 | CHANGE (new) | Quantize, triplets, swing | Q and ⌘3; snap down to 1/64. | — | later #15 |
| NT-8 | CHANGE | Note colours | From the palette by velocity; the selection as in §2.9. | `e04` | T |
| NT-9 | KEEP | Note names on notes, velocity lollipops, marquee, arrow nudges (⇧ for an octave or a bar), the length field, the END hatching, the empty-state line | | `e03`, `c22` | — |

### ST: Status, problems and toasts (`d04`, `d05`, `d09`–`d12`, `d15`, `d16`, `f10`, `g02`, `c03`, `c13`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| ST-1 | CHANGE (new) | One status pill and popover | The pill shows the most severe state, adding the others as counts: "Reconnecting…" (danger, `WifiOff`), then "Code doesn't compile" (warn, or danger if nothing plays), then "2 problems" (by the highest severity), then "Read only" (neutral, `Lock`). Nothing else repeats it: no edge line under the bar. The 360 px popover lists each item with its fix buttons; its codes are behind "Details" (mono `text-xs` `fg-faint`). | `d05`, `d09`, `d10`, `d15`, `f10` | S |
| ST-2 | CHANGE | Read-only, said once (D5) | The pill only: no Needs attention row and no refused-edit toast. Edit affordances show values without drag cursors, and fields are disabled. Copy: "Open in another window or in m2. You can listen here; to edit, close it there, then Retry." No pid and no "run this again". | `d15`, `d16` | S |
| ST-3 | CHANGE | Compile errors | Pill plus popover ("tracks/keys.ts:7: Unexpected token · Show in code"). Keep the locked-knob reason in menus. | `d05`, `d06`, `g02` | S |
| ST-4 | CHANGE | Needs attention fixes | They move into the popover. Keep "Move all to Kick (kicks)" as a primary button. Replace the native `<select>` with a "Move to…" menu button. | `d09`, `d10`, `d12` | S |
| ST-5 | CHANGE | Toasts | As in §2.14: only for results off screen or structural, in words from descriptors with Undo: "Added Reverb after Keys", "Deleted Kick and 2 Ducks", "Undid adding Lead Vox". A direct edit ("Hats volume −6.0 → −3.0 dB", a rename) is words in the Undo tooltip and Edit ▸ Undo, not a toast. | `c03`, `c04`, `c13`, `c14`, `c17`, `d14` | S (look); each 8b builder writes its own words |
| ST-6 | REMOVE | Code diffs and internal codes in messages | "song.ts: -6 → 12" and `data.unclaimed` go to Details and the code view. | `c13`, `d09` | S + editor messages |
| ST-7 | CHANGE | Lost connection | The `danger` pill; editing is disabled until reconnected. Not a red capsule over the timeline. | `f10` | S |

### KN: Knobs, faders and menus (`c01`, `c02`, `c05`, `c06`, `d06`, `f07`, `f08`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| KN-1 | CHANGE | Pro gestures | Use the gesture table in §5.2. The full range takes 300 px of drag (160 today); ⇧ gives ×0.1. | `c02` | T |
| KN-2 | CHANGE | Knob menu | Standard rows: Type a value… ⏎ · Automate · Reset to default ⌥-click · Copy value ⌘C · Paste value ⌘V · — · Edit in code (with the file:line detail). On a knob a lane drives: Show Cutoff lane · Remove automation (8b), and a drag shows "Follows the Cutoff lane". Once latches exist: Re-enable automation · Write to code. No tiles and no two-line descriptions. | `c05`, `c06` | UI; 8b:clips (lane-driven rows) |
| KN-3 | CHANGE (new) | Right-click menus everywhere | Track headers (§3.4); clips (Cut, Copy, Paste, Duplicate, Split, Join, Delete, Loop this clip); lanes (Add clip, Clear, Hide lane); notes (Velocity…, Delete); strips. | — | UI (gaps #17) |
| KN-4 | CHANGE | One popover style | As in §2.14, opening at the trigger, not at the pointer. Today the knob menu covers the view switcher (`c05`). | `c05`, `c11` | T |
| KN-5 | KEEP | Disabled with a reason while the code is broken | | `d06` | — |

### BR: The browser and effect picker (`c09`–`c13`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| BR-1 | CHANGE (new) | The side browser | As in §3.6, including dragging onto tracks, Device and empty space. | — | S (list); 8b:tracks (dropping onto empty space) |
| BR-2 | CHANGE | Names are labels | "Low-pass", "Reverb"; the code name is in the tooltip. | `c09` | T |
| BR-3 | CHANGE | Search finds by use | Matches the label, the description and `editor.keywords` (std adds them: Duck gets "sidechain" and "pump"). | `c12` | UI + std |
| BR-4 | REMOVE | The icon and colour on every item, the "esc" key cap and the "↑↓ to move · ↵ to add" hint | One icon per category header. | `c09` | T |
| BR-5 | KEEP | Typeahead plus Enter adds; the descriptions; the code preview at the foot (literal code, mono `text-xs`) | | `c09`, `c11` | — |

### CD: Code (`b13`, `b14`, `d03`–`d08`, `e07`, `e08`, `e12`, `f01`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| CD-1 | CHANGE | No stale diagnostics after switching projects (D1) | Dispose of every Monaco model and its markers when the session changes (the models are global by path). Must land before 8b's track builders, which create files and switch modules. | `e12` vs `f01` | S |
| CD-2 | CHANGE | The status line holds the revision | "Compiles · playing rev 2". | `b13` | T |
| CD-3 | CHANGE | Theme from tokens | §2.15's mapping; `ui-monospace` at 13 px. | `e08` | T |
| CD-4 | CHANGE | Opens on the selected track's file; ⌥4 | | `b13` | S |
| CD-5 | KEEP | Types, ⌘S, completions, changed-on-disk, the problems strip | Restyled with the status tokens. | `d03`, `d08`, `e08` | — |

### EX: Export (`d13`, `d14`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| EX-1 | KEEP | One-click Export with the progress ring in the button; you keep working while it renders | | `d13` | — |
| EX-2 | CHANGE | The done toast reports the result | "Exported My Song.wav · 0:16 · peak −0.8 dB", with "Show in Finder" as its action (the event carries `peak_db`). | `d14` | UI |
| EX-3 | CHANGE (new) | Warn before exporting with overlays on (MON-4) | A dialog: "Kick is muted only while you listen. It will be in the file." [Export anyway] [Mute Kick in the song, then export]. The second writes the MON-5 option on every muted track in one Tx, then exports; its done toast says Kick stays muted in the song and offers Unmute Kick. "Don't ask again for this song" (kept in `view.json`) skips the dialog, and the done toast then carries the warning. The export with monitoring mutes still carries `overlays.active`. | — | 8b:mixer |
| EX-4 | CHANGE (new) | An "Export…" sheet | Range (song, loop, selection), stems, format, bit depth, normalize, tail. | — | later #36 |

### SS: The start screen (`a01`–`a03`, `g01`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| SS-1 | REMOVE | The logo tile, the "music2" heading and the gradient New Song button | | `a01` | T |
| SS-2 | CHANGE (new) | Recent songs | A 560 px column. Each row is 44 px: name (`text-base` 500), "124 BPM · opened yesterday" (`text-xs` `fg-muted`) and a 96×28 px arrangement thumbnail in `bar` colours. Enter opens the newest. | `a01` | later #12 (small: pull it into step 8) |
| SS-3 | CHANGE | Three actions in a row above the list | New Song… (primary, ⌘N), Open… (secondary, ⌘O; native only), Examples ▾ (menu). The folder hint goes underneath in `text-xs` `fg-faint`. | `a01`, `a03` | T |
| SS-4 | KEEP | The examples' copy-first sentence, their descriptions, and the With plugins refusal | Their 28 px icon tiles go. | `a03`, `g01` | — |

### NS: The New Song dialog (`a04`–`a08`)

| # | Do | What | Spec | Shots | Lands with |
|---|---|---|---|---|---|
| NS-1 | KEEP | ⌘N, type, Enter: the fastest flow in the app | | `a04`–`a06` | — |
| NS-2 | CHANGE | Flat and in the system | `rounded-md` dialog, `rounded-sm` inputs, a flat `accent` Create button, an `accent` radio check, thumbnails 48 px tall in `bar` colours. | `a04` | T |
| NS-3 | CHANGE (new) | Tempo at creation | A "Tempo" field next to the name (the template's BPM by default), written into the new song's `arrangement.json`. | `a04` | 8b:tempo |

---

## 5. Keyboard shortcuts

The music2 column is the decision. The Logic and Live columns are the conventions it follows, from
their US default key commands. A dash means there is no default I'd rely on. Check them against the
current manuals before printing a cheat sheet. Every ⌘ and ⌃ shortcut is a menu item in `menu.rs`,
because the native menu bar owns those chords (`App.tsx` already defers to it). Everything else (single
keys, Space, Return, ⌫, arrows, ⌥ chords) is handled by the page, with no native accelerator (§5.3).

### 5.1 The map

| Action | music2 | Today | Logic Pro | Ableton Live |
|---|---|---|---|---|
| **Transport** | | | | |
| Play / stop (stop returns to where play started) | Space | Space (toggle) | Space | Space |
| Continue from where it stopped | ⇧Space | — | — | ⇧Space |
| Go to start | Return, Home | Enter, Home | Return | — |
| Loop on / off | C | button | C (Cycle) | — |
| Loop the selection | ⌘L | ruler drag | ⌘U (locators from the selection) | ⌘L |
| Metronome | K | — | K | — |
| Record (reserved) | R | — | R | F9 |
| Follow the playhead (on by default, kept per song) | ⇧⌘F | — | — | ⇧⌘F |
| Stop all sound (panic) | ⌘. | — | — | — |
| **Edit** | | | | |
| Undo / redo | ⌘Z / ⇧⌘Z | ⌘Z / ⇧⌘Z | ⌘Z / ⇧⌘Z | ⌘Z / ⇧⌘Z |
| Cut / copy / paste | ⌘X / ⌘C / ⌘V | text fields only | ⌘X / ⌘C / ⌘V | ⌘X / ⌘C / ⌘V |
| Duplicate (clip, track or notes: whatever is selected) | ⌘D | ⌘D (notes only) | ⌘R (repeat) | ⌘D |
| Delete | ⌫ | ⌫ (notes, clip) | ⌫ | ⌫ |
| Split at the playhead | ⌘E | — (⌘E is Export) | ⌘T | ⌘E |
| Join clips | ⌘J | — | ⌘J | ⌘J (Consolidate) |
| Rename | ⌘R, ⇧Return | double-click | ⇧Return | ⌘R |
| Select all / deselect | ⌘A / Esc | ⌘A (notes) / Esc | ⌘A | ⌘A |
| Draw mode | B | — | T → Pencil | B |
| Quantize | Q | — | Q | ⌘U |
| Nudge by the grid / by a bar | ← → / ⇧← ⇧→ | notes only | ⌥← ⌥→ | ← → (notes) |
| Transpose a semitone / an octave | ↑ ↓ / ⇧↑ ⇧↓ | notes only | ⌥↑ ⌥↓ / ⇧⌥↑ ⇧⌥↓ | ↑ ↓ / ⇧↑ ⇧↓ |
| **Tracks** | | | | |
| New track | ⌘T | — | ⌥⌘N | ⌘T (audio), ⇧⌘T (MIDI) |
| Delete track | ⌘⌫ | — | ⌘⌫ | ⌫ |
| Mute the selected track | M (also ⌃M, Mac only) | — | ⌃M | — |
| Solo the selected track | S (also ⌃S, Mac only) | — | ⌃S | — |
| Show / hide automation | A | — | A | A |
| Track height | ⌘↑ / ⌘↓ | — | ⌘↑ / ⌘↓ | — |
| **View** | | | | |
| Zoom in / out | ⌘= / ⌘−, pinch | — | ⌘→ / ⌘← | + / − |
| Zoom to the selection, or back | Z | — | Z | Z (X goes back) |
| Narrower / wider grid | ⌘1 / ⌘2 | — | — | ⌘1 / ⌘2 |
| Triplet grid | ⌘3 | — | — | ⌘3 |
| Snap on / off | ⌘4 | — | — | ⌘4 |
| Bypass snap while dragging | hold ⌘ | — | — | hold ⌘ |
| Device / Notes / Mix / Code | ⌥1 / ⌥2 / ⌥3 / ⌥4 | tabs | B, P, X (Smart Controls, Piano Roll, Mixer) | ⇧Tab toggles Clip/Device |
| Mix | X (also ⌥3) | — | X | ⌥⌘M |
| Show / hide the dock | E | — | E | ⌥⌘L |
| Browser | ⌥⌘B | — | Y (Library) | ⌥⌘B |
| Command palette | ⇧⌘P | — | — | — |
| Keyboard shortcuts | ⌥K | — | ⌥K (Key Commands) | — |
| Musical typing (reserved, gaps #24) | ⌘K | — | ⌘K | M |
| **File** | | | | |
| New song / open | ⌘N / ⌘O | ⌘N / ⌘O | ⌘N / ⌘O | ⌘N / ⌘O |
| Close song | ⌘W | ✕ button | ⌘W | — |
| Save (code view) | ⌘S | ⌘S | ⌘S | ⌘S |
| Export audio | ⇧⌘E | ⌘E | ⌘B (Bounce) | ⇧⌘R |
| Settings | ⌘, | — | ⌘, | ⌘, |

**Choices worth explaining:**

- **⌘E splits, as in Live.** Logic's split key, ⌘T, makes a track in Live, and music2 follows Live
  for track and clip editing.
- **Export moves to ⇧⌘E.** The name is a mnemonic, and the key is free in both DAWs. Live users
  will look in File, where it is listed.
- **⌘1 to ⌘4 keep Live's grid meaning.** Grid changes are frequent while writing notes, so the dock
  views go on ⌥1 to ⌥4. The timeline and the roll each have a grid; ⌘1 to ⌘4 act on the one that
  last had the pointer or the keyboard focus.
- **The shortcut sheet is ⌥K, as in Logic.** ⌘/ stays the code editor's comment.
- **⌘K stays free for musical typing,** as in Logic. The command palette takes ⇧⌘P, the code
  editor convention, which fits a code-first app.
- **M and S mute and solo the selected track.** This is fast and one-handed. ⌃M and ⌃S also work for
  Logic hands, on a Mac only: elsewhere Ctrl stands for ⌘, and Ctrl+S saves. Live's M (computer keyboard) is covered by ⌘K.
- **Stop returns to where play started (Live).** ⇧Space continues from where it stopped. A click in
  the ruler moves the start point; so does a click on empty lane space, while stopped only (8b.2): a
  click to deselect while playing never makes the song jump, and no start point moves unseen.
- **No ⇧Tab:** it moves keyboard focus backwards, and focus traversal must keep working.

### 5.2 Gestures

| Target | Gesture | Result |
|---|---|---|
| Knob, fader, tempo field | drag (vertical; a fader along its travel) | coarse: the full range over 300 px; the tempo field moves 1 BPM per 4 px |
| | ⇧-drag | fine, ×0.1 (Live) |
| | ⌥-click | knobs and faders: reset to the default (Logic). Not the tempo field, where a stray click would reset the song's tempo |
| Fader | its foot (the last 4 px) | −∞ (`db: -96`, std's silence) |
| | double-click, or Return when focused | type a value (§2.14) |
| | scroll or two-finger swipe | one step per notch: 1% of a linear range, a 1% ratio for Hz, 0.1 dB, 1 semitone |
| | ↑ ↓ when focused / ⇧↑ ⇧↓ | one step / ten steps |
| | ⌫ when focused | reset (Live) |
| | right-click | the knob menu (§4 KN-2) |
| M / S | click | toggle (a solo adds to the other solos) |
| | ⌥-click S | exclusive solo: only this track |
| | Clear all, in the overlay pill's popover | clear every mute and solo (the pill has no ×) |
| Clip | drag the body / either edge | move / resize |
| | ⌥-drag | copy |
| | double-click | open in Notes (notes clips) |
| Lane (empty) | click | while stopped: move the playhead there, as the ruler does; while playing: nothing |
| | double-click | a new 1-bar clip (keep) |
| Automation clip | click or drag within 6 px of the curve / click or drag elsewhere on the clip | add a point and move it / select or move the clip, as for notes clips |
| Ruler | click / drag | set the start point / set the loop |
| Notes grid | the click rules in §3.5 | |

### 5.3 Rules

- **Keys per OS (OS-54).** The map writes the Mac's keys. Elsewhere ⌘ is Ctrl, ⌥ is Alt, ⇧ is Shift,
  ⌫ is Backspace and Return is Enter, and every label spells them in words: "Ctrl+Shift+E",
  "Shift-drag", "Alt-click". `web/src/lib/native.ts` spells them all (`chord`, `opt`, `shifted`,
  `keyName`, `mod`); no label writes a key by hand. ⌃ chords are Mac only. In a text field,
  Ctrl+Backspace deletes the word before, as PC fields do (⌘⌫ deletes to the line's start on a Mac).
- **Native menu accelerators are only ⌘ and ⌃ chords.** A native accelerator fires even while the
  producer types in the code editor or a rename field, which is why `menu.rs` already leaves Space
  out. So single keys, Space, Return, ⌫, arrows and ⌥ chords are the page's, and are ignored while
  focus is in a text field or the code editor.
- **Focus decides what a ⌘ chord does.** When the code editor has focus, a chord that Monaco binds
  (⌘D, ⌘L, ⌘., ⌘E, ⌘⌫, ⌘↑, ⌘↓) runs Monaco's own action, as undo already does. In a rename or
  type-in field, ⌘⌫, ⌘↑ and ⌘↓ do the field's edit. Everywhere else the chord is the menu's command.
- Every ⌘ and ⌃ shortcut appears in its menu row. Every shortcut appears in its tooltip ("Loop (C)")
  and in the ⌥K sheet, grouped as in §5.1.
- The Transport menu lists Play, Continue, Go to Start, Loop, Loop Selection, Metronome and Follow.
- The View menu lists Zoom In, Zoom Out, Zoom to Selection, Track Height, Show Automation, the four
  dock views, Show Dock, Show Browser, Command Palette and, on a Mac, Full Screen.
- The Track menu (new) lists New Track, Delete, Rename, Mute (⌃M on a Mac), Solo (⌃S on a Mac) and Mute
  in Song.
  Duplicate is one item, in Edit, and acts on whatever is selected (§5.1).
- The Help menu lists Keyboard Shortcuts. Off the Mac there is no app menu: About ends Help, and Quit
  (Ctrl+Q) ends File. Only the Mac has a Window menu.

---

## 6. What to delete

Delete each of these outright; don't restyle them. File pointers are at HEAD `cbf5eb9`.

**Fonts and global CSS**
1. `web/index.html`: the Google Fonts `preconnect` and stylesheet links (D10).
2. `web/src/index.css`:
   - `--color-ink-*`, `--color-pop-*`, `--color-violet-glow` and `--color-sun`;
   - the `.glass`, `.pop-gradient` and `.pop-text` classes;
   - the Inter `font-feature-settings`;
   - the mauve `::selection`.
3. Every `backdrop-*` (5 uses) and `transition-all` (in `ui.tsx` and others).

**Arbitrary values**
4. Every arbitrary text size: 80 `text-[Npx]` uses in 10 sizes. Map 9, 10 and 10.5 to `text-xs`,
   12.5 and 14 to `text-base`, and 19 to `text-lg`. The 30 px start-screen title goes (SS-1).
5. Every radius outside `xs`, `sm`, `md` and `full`: the `rounded-xl`, `-2xl`, `-lg`, `-md` and
   `rounded-[3/7/9/10/20/22px]` uses.
6. `uppercase` and `tracking-*` (the "TRACKS" label and the browser's category labels).
7. `font-mono` on anything that isn't literally code (30 uses today; about 8 remain): lane names,
   node titles, the position, the BPM, "rev", the mixer's row names.
8. `italic` as "default" (3 uses), replaced by §2.10's markers.

**Buttons**
9. `ui.tsx`:
   - the `pop` variant, replaced by `primary`;
   - `rounded-xl`, `active:scale-[0.98]`, `shadow-lg shadow-pop-500/10` and `hover:brightness-110`;
   - `disabled:opacity-40`, replaced by explicit disabled colours.

**Top bar and shell**
10. `GraphCanvas.tsx`:
    - the logo tile (lines 433–435);
    - the gradient play button and pause glyph (436–445);
    - `rev {revision}` (462);
    - the close button (484–486);
    - `h-16` and `pl-[88px]` (432), replaced by 44 px and 80 px;
    - the compile banner and read-only banner components (266–335), replaced by the status pill;
    - the module tab row.
11. `App.tsx`:
    - the gradient "music2" loading title (it becomes "Connecting…" in `text-sm` `fg-muted`);
    - the red lost-connection capsule at `top-[76px]`;
    - ⌘E → export.
12. `Attention.tsx`:
    - the strip that pushes the layout;
    - the native `<select>`;
    - visible diagnostic codes;
    - the `lock.held` item (read-only is the pill).
13. `Toasts.tsx`: the right-side stack, `rounded-2xl glass shadow-2xl` and `line-clamp-3`.

**Timeline**
14. `SongTimeline.tsx`:
    - "TRACKS" (325) and the meter/BPM corner label;
    - the "track" and key captions;
    - the lane-kind and note-count labels;
    - `mute()`, `COLORS`, `MIX`, index-based `colorOf`, `trackColor` and `trackHue` (42–72),
      replaced by `palette.ts` and `layout.json`;
    - the playhead's `transition-transform duration-200` and `bg-sun` glow (333, 349);
    - the track-coloured dashed fallback lines across whole lanes.

**Graph, browser, roll and editor**
15. `NodeGraph.tsx`:
    - the legend row and the "Drag a named output…" hint (527–534);
    - `KIND_COLOR` (170);
    - the file:line subtitles;
    - the outer card;
    - the zoom bar when the graph fits.
16. `EffectBrowser.tsx`: the per-category colours and per-item icons (`CATEGORIES`, 12–20), the
    "esc" key cap and the move/add hint.
17. `NoteView.tsx`:
    - hard-coded `#2b2d32`, `#1f2126` and the `rgb(143,101,116)` fallback (palette and tokens
      instead);
    - the note count, "New notes 1/16" and the ✕.
18. `NewSongDialog.tsx`: the saturated `HUES` (236), the gradient radio and Create button, and
    `rounded-[22px]`.
19. `StartScreen.tsx`: the logo tile and the gradient New Song button.
20. `codeMonaco.ts`: every `#8f6574`, `#7c5563` and `#7f7a92` (mauve and violet) and the `#c9a54e`
    find colour, plus the JetBrains Mono font family.
21. `menu.rs`: `CmdOrCtrl+E` on Export (it becomes `CmdOrCtrl+Shift+E`; ⌘E goes to Split once clip
    editing lands).

**Copy**
22. Every user-facing string that prints:
    - a code diff ("tracks/keys.ts: pluck(…) → reverb(…)");
    - a pid;
    - "run this again";
    - a slot path (`t01.db`);
    - a binding name (`out-decay`).

### 6.1 Don't delete these: they are the good parts

- **Timeline:** double-click to make a clip; the `lit` flash on a new lane.
- **Roll:**
  - note names drawn on notes;
  - the velocity lollipops;
  - the marquee;
  - arrow nudges;
  - the "Click in the grid…" empty-state pattern;
  - the END hatching.
- **Effect picker:** typeahead plus Enter; the plain-English effect descriptions; the code preview
  at its foot.
- **Messages:**
  - "Move all to Kick (kicks)" and the other one-click fixes;
  - the disabled-with-a-reason menus while the code is broken;
  - the With plugins refusal copy.
- **Flows:** the ⌘N → type → Enter New Song flow; the progress ring inside the Export button.
- **The code view's Monaco setup:** types, completions, changed-on-disk and ⌘S.
- **The Chords device chain layout** (`e02-…-chords`): it is the template for the Device view.

---

## 7. Attack response

The problems raised against design-8b revision 1 that reach the UI changed this file too. Their ids are
design-8b's (its §18 has the full table).

| # | Problem | Verdict | Why | Where it changed here |
|---|---|---|---|---|
| B2 | "Change key…" in the track menu reads as a musical key | **Accept** | Every musician reads "key" as C minor, and the top bar plans a Key field. A module key is the track's "ID" in all copy, offered only in the Code view's file tree | Words table, §3.5 Code, principle 3 |
| B4 | Native menu shortcuts take keys away from the code editor and text fields | **Accept** | A native accelerator fires while typing. Only ⌘ and ⌃ chords are accelerators; focus sends Monaco's chords to Monaco; the sheet moves to ⌥K; Duplicate is one Edit item | §5 intro, §5.1, §5.3 |
| B5 | A toast after every fader, knob, tempo and clip edit, drawn over the Mix strips | **Accept** | Toasts only for results off screen or structural; the words name the undo step; toasts sit above the dock | principle 3, §2.14, ST-5, TL-5 |
| B6 | A channel's first pan is silent until release | **Accept** | The pan is written as the drag passes 4 px and is heard during the drag | §3.5 Mix |
| B7 | On automation clips, a click both adds a point and selects the clip | **Accept** | Points within 6 px of the curve; elsewhere the clip behaves as a notes clip | TL-6, §5.2 |
| B8 | Duplicating a section lands off the bar grid | **Accept** | A span of a bar or more duplicates onto the next bar; a marquee keeps its own bars | TL-5 |
| B9 | Drawn automation makes its knob dead | **Accept the minimum** | The drag says why; the knob menu has Show lane and Remove automation; latches later | §2.10 lane row, KN-2 |
| B10 | After + Track, the rename field takes Space, M and S | **Accept** | No rename opens; the track is selected and lit; ⌘R renames; the list opens on the last instrument added | TL-3 |
| B11 | The fader's floor is −60 dB, not −∞ | **Accept** | The foot writes −96, std's own silence, and reads "−∞" | §3.5 Mix, §5.2 |
| B12 | The header level slider is imprecise, a duplicate, and squeezes the name | **Accept** | Gone; the name gets 140 px; owner question Q1 confirms the taste call | principle 4, §3.1, §3.4, TL-1 |
| B14 | Hollow "default" markers on most values | **Accept** | Inherited values live in code; `fg-muted` text and a tooltip say it; every remaining marker means something | principle 7, §2.10, §3.1 key, §3.5 Mix |
| B15 | The export dialog appears on every export and doesn't say it edits the song | **Accept** | "Don't ask again for this song"; the button names the lasting change; the done toast offers Unmute | EX-3 |
| B16 | The overlay pill's × clears every mute and solo with no undo | **Accept** | Clear all moves into the popover; no pill has a × | §2.14 pills, §3.3, TB-7, §5.2 |
| B17 | Mute in song with solo, the M key and drags across M unspecified | **Accept; mute keeps winning** | S's tooltip says why it's silent; the M key and drags skip a track muted in the song | §2.9 |
| B18 | Decoration and extra subsystems | **Accept** | Cut here: the status edge line (the pill says it), the M button's code dot (the strikethrough and hatching say it), tap tempo, the tempo field's ⌥-click reset (a stray click would reset the song), and the pulsing dot on MON-2's row | principle 8, §2.6, §2.9, §3.1, §3.3, §3.5, ST-1, ST-3, ST-7, MX-3, §5.2 |
| B21 | Mix view gaps: shared strips, and the module MON-2's row names | **Accept** | A shared strip's M and S act on all its tracks, with a dash when they differ. The row reads "Still drives Duck on Bass" (labels and the channel's track), settling revision 1's open wording | §2.9, §3.5 Mix, MX-3 |
| B22 | Selecting a track can swap the dock from Mix to Device | **Accept** | The dock's view is one choice for the window; selection changes only its content | §3.5 Device, §3.7 |
| B23 | Code the builders leave behind reads wrong or mixes styles | **Accept** | A first fader move writes `db(-3)` beside `db(-6)` | MX-2 |
| B24 | Inconsistencies between this file and design-8b | **Accept** | 1 BPM per 4 px; EX-3 is one Tx; ⌘1 to ⌘4 act on the editor last pointed at or focused; Follow on by default | §5.1, §5.2, EX-3 |
| B25 | Join is missing | **Accept** | ⌘J joins notes clips, as Logic's Join and Live's Consolidate | §5.1, TL-5, KN-3 |

---

## 8. Build slices

Every slice runs the checks in §0. The 8b items land in the order design-8b §19 gives.

| Slice | What | Touches | When |
|---|---|---|---|
| **T: tokens** | The `@theme` block in §2.15; the system font; one accent; no gradients, glass or blur; four type sizes; three radii; port shapes instead of port colours; track colours from the palette. Mechanical, with no behaviour change. | `web/src/index.css`, `web/index.html`, `ui.tsx`, class names everywhere, `codeMonaco.ts` | First, before any 8b UI |
| **S: shell** | 44 px top bar; the dock with Device/Notes/Mix/Code in place of the module tabs, its view one choice for the window (§3.7); one status pill in place of the banners, with no edge line; toasts above the dock, only for structural results (§2.14); track headers without captions or a level slider; the side browser; native accelerators only for ⌘ and ⌃ chords, with focus routing in `appCommand` and the ⌥K sheet (§5.3); D1, D2, D4, D12. | `GraphCanvas.tsx`, `SongTimeline.tsx`, `Attention.tsx`, `Toasts.tsx`, `App.tsx`, `menu.rs`, a new `Dock.tsx` and `Browser.tsx` | Second |
| **8b.1 tracks** | + Track and its list (TL-3, BR-1), the header menu, colours by name (§2.7), the Empty template's Add a track | as the item needs | with design-8b's 8b.1 |
| **8b.2 tempo bar** | the tempo and meter fields (§3.3, TB-5), the title, New Song's tempo (NS-3) | | with 8b.2 |
| **8b.3 mixer** | the Mix view and its faders with a −∞ foot (§3.5, MX-1, MX-2, MX-5, MX-6) | | with 8b.3 |
| **8b.4 mute and solo** | M and S and their states (§2.9), the overlay pill and its popover (TB-7), MON-2's row (MX-3), mute in song (MX-4), the export dialog (EX-3) | | with 8b.4 |
| **8b.5 clips** | clip gestures (TL-5), automation in place (TL-6), the grid chip, zoom and heights (TL-11), the End flag (TL-12), markers, the knob menu's lane rows (KN-2), right-click menus (KN-3) | | with 8b.5 |
| **8b.6 audition** | the audition toggle and the playable keyboard (NT-2, NT-3), the roll's hints | | with 8b.6 |
| **8b.7 leases** | the tempo number in `accent` while its lease lives | | with 8b.7 |
| **8b.8 meters** | header, strip and master meters (§2.8), the DSP pill (TB-8) | | with 8b.8 |
| **8b.9 IDs** | Change track ID… in the Code view's file tree (§3.5 Code) | | with 8b.9 |

---

## 9. Owner questions

Only taste decisions this file can't settle alone. Design-8b's Q4 asks about the cut instrument
previews and tap tempo.

| # | Question | This file's answer |
|---|---|---|
| Q1 | Track headers carry a colour, a name, M, S and a meter, and no level slider: the level lives only on the Mix strip, ⌥3 away. Agreed? | Yes. A 44 px slider can't be precise, it would be a second control for one value, and the name needs the room ("Shakers & Hats"). If you miss it while arranging, it can come back as a relative 300 px drag with ⇧ for fine, never a jump to the click |
