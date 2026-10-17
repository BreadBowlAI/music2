# Build notes: polish

The polish track, by part. Each part writes only under its own heading.

## P0

The lead's seams.

## Lane A

Mix strips, change instrument, drum names, track IDs. Branch `polish-a`, worktree `wt/pl-a`.

### A1 and A2: M, S and a meter on every Mix strip

**In short**

| What | Now |
|---|---|
| M and S on a strip | Each track's strip has M and S, centred under its pan. They are the header's buttons: the same state, look and tooltips |
| A strip of two tracks ("Kick + Hats") | M and S act on both. When the two differ, the button shows a dash. ⌥-click on S solos exactly those two |
| The strip's menu | It has Mute in song (or Unmute in song, first, when the strip's tracks are muted in the song). One undo step, with a toast |
| A meter on each strip | Two thin bars, left and right, beside the fader. A track's meter shows its sound before the fader, so moving the fader doesn't move it |
| Master's meter | Two wider bars: what the song sends out |
| The clip dot | A small red dot over a track's meter when its sound goes over the top (0 dB). It stays until you click it. Master has none: it ends in a limiter |

**What changed**

- `MuteSolo.tsx`: one `Pair` of buttons for a list of tracks. The header passes one track; `StripMuteSolo` passes a
  strip's tracks. A button's state is on, off or `mixed` (`aria-pressed="mixed"`, drawn as a dash in the on colour).
  A click turns all on when any is off. A drag across M buttons crosses headers and strips alike. A strip's buttons
  carry `data-strip-mute` and `data-strip-solo` (a header's keep `data-mute`, `data-solo`), so header suites don't see
  two buttons per track. A strip of several tracks names no key in its tooltips (the M key acts on the selected track).
- `lib/overlays.ts`: `soloOnly(modules)` solos exactly a list (`exclusive` the first, `solo` the rest).
- `lib/meters.ts`: each meter keeps three levels (the louder side, left, right). `mount(keys, canvas, bars)` draws
  one bar or two, the louder of several tracks per side. Every meter's readout (`aria-valuenow`, and `data-left`,
  `data-right` on a two-bar one) changes on the same frame, 10 times a second, so two meters of one track always say
  the same. A track's peak at 1.0 or more latches its clip (`useClipped`, `clearClips`); a new song clears them.
- `Meter.tsx`: `channels` and `bars` props, a `title`, and `ClipDot` (a 12 px button holding a 4 px `danger` dot; no
  focus, so Space still plays).
- `Mix.tsx`: the M and S row; `StripMeter` in the fader's column (a drag on it still moves the fader), 7 px wide
  (11 on Master), from 9 px under the travel's top to its foot, the clip dot above it. A strip whose level is set in
  code has the meter too, with its words under it. The strip menu: Mute in song, through `muteInSongCall` for all the
  strip's tracks.

**Tests**

| Suite | Checks | Result |
|---|---|---|
| `pl-mix` (new, after `s8-mix`) | 47 | ok |
| `s8-mix` | 80 | ok. Two checks follow the change: Tab now goes pan, M, S, fader; the strip menu has Mute in song (4 rows) |
| `s8-mute` | 152 | ok |
| `s8-meters` | 24 | ok |
| `taste` | 39 | ok |

Web build: ok. No Rust changed.

**Red first** (the new suite on the page from before, `29430e8`):

```
FAIL  the Kick strip has M and S, as every track's strip does ([["kick",false,false],["hats",false,false],…])
FAIL  the Hats strip has a meter, as every strip does (kick:0 hats:0 bass:0 keys:0 master:0)
FAIL  the Hats strip's two bars both read above the floor
```

(The second and third come from a run of the meters part alone: on the old page the M and S part stops the suite.)

**Notes**

- First-song's Bass reaches 0 dB where it enters the duck, so its clip dot lights while the song plays. That is true
  of its sound; the master's limiter keeps the song itself clean.
- `pl-mix` lints through `e2e/lintlib.mjs` (P0b-1's). It runs as `page.evaluate(lint, sel)`.

**What's left**

- Meters after the fader (Q1: needs taps in `core/runtime`).
- The "Still drives Duck on Bass" row on a muted strip (taste.md §3.5 MON-2): the words are in M's tooltip, as on the
  header; the strip doesn't dim.

**For gaps.md:** the Mix view's strips have M, S and meters (pre-fader), and clip dots. Left: post-fader meters.

**Contract:** none.

## Lane B

Timeline.

## Lane C

Songs.

## Lane D

Notes and automation.

## Merge

The lanes into `polish`.

## End

The full check, docs and pictures.
