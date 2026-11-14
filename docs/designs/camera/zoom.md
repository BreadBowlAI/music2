# Zooming at the screen's rate

| | |
|---|---|
| **Status** | Proposed, 2026-11-14. Waiting for the owner's yes on Z1 (§4). |
| **Written** | 2026-11-14, from `main` at `e06d0bfc`, after the owner's swipe probe: a pinch over the tracks had 69 frames over 16.7 ms. The owner asked earlier: "why does frame rate drop significantly when i zoom in on the tracks or notes editor". |
| **Beside it** | [design.md](design.md), the camera note (VIEW-1 to VIEW-12). This note changes VIEW-5, VIEW-8 and VIEW-12. |
| **Measured on** | Branch `pinch-cost` (not for merge): a pinch bench in Chrome and in WebKit (WebKitGTK 2.52, the engine the Mac app uses), on a beat song like the owner's (10 tracks, 312 clips) and on the big song (24 × 64 clips). |

**In short**
- **Why a zoom is slow:** each frame of a zoom moves and redraws every clip the tracks have drawn, and every note inside them. In WebKit, a pinch frame on a beat song takes 38 ms of work. A 120 Hz screen gives 8.3 ms.
- **What costs most:** the clips' small drawings of their notes (about a third to two thirds of a frame), the clip boxes (a fifth), and the rest of the tracks' view. React itself is only about a tenth.
- **No small fix is enough.** Taking all the clips' drawings away still leaves 26 ms in WebKit.
- **The fix, in two parts:**
  - **Z1, a live zoom.** While the fingers zoom, the browser stretches the picture it already has, which costs almost nothing. The real redraw happens once the zoom ends. In WebKit, a beat song's pinch frame goes from 38 ms to 8 ms. The cost: names, bar numbers and borders stretch for a moment while you zoom. **This needs the owner's yes.**
  - **Z2, cheaper redraws.** Clips draw their notes as pictures (a canvas) rather than as hundreds of shapes, and the tracks draw a narrower margin. This makes every real redraw a quarter to a half cheaper: the end of a zoom, ⌘=, Fit, and new clips coming into view while you swipe. Nothing looks different.

## 1. What was measured

The bench sends a pinch the way WebKit sends one: `gesturestart`, then a `gesturechange` every 8 ms (the owner's A13 shape: scale 1 → 0.44 → 1), then `gestureend`. It goes through the camera's real listeners, recognizer, store, React and writer. It does the same with ⌘-scroll.

"Work" is the main thread's time for one frame. The screen allows 8.3 ms at 120 Hz.

| Song, engine | Work per frame, median (ms) | Frames over 8.3 ms |
|---|---|---|
| Beat song, WebKit | 38 | 14 of 15 |
| Big song, WebKit | 73 | all |
| Beat song, Chrome | 14 | 47 of 56 |
| Big song, Chrome | 39 | all |

Where a slow WebKit frame's time goes, on the beat song:

| Part | Share |
|---|---|
| Style and layout | 41% |
| Paint and handing the layers over | 36% |
| Finding what is under the fingers, per event | 13% |
| JavaScript (React, the camera) | 10% |

Taking parts away, one at a time (median work, ms):

| Song, engine | As it is | No note drawings | No clips at all | No grid |
|---|---|---|---|---|
| Beat, WebKit | 38 | 26 | 18 | 36 |
| Big, WebKit | 88 | 57 | 33 | 86 |
| Beat, Chrome | 15.6 | 6.7 | 4.6 | 15.5 |

A clip's drawing as a picture the browser scales (a stand-in for a canvas) cost nearly as little as no drawing at all: beat, WebKit 28 ms; Chrome 7.1 ms.

WebKit here drew in software on a server, not on a Mac's graphics chip. Its main-thread work (style, layout, paint recording) is the same engine code as on the Mac. My estimate for the Mac is 0.7 to 1.3 times these numbers, and the owner's probe fits that: the slowest frame of their pinch was 79 ms.

## 2. Z1: a live zoom

**What it does.**
- While a pinch or a run of ⌘-scroll lasts, the camera doesn't lay the tracks out again on each frame. It sets one `transform` on the tracks and the ruler: a stretch across, around the point under the fingers. The browser stretches what it already drew, on the graphics chip.
- When the zoom ends (the gesture ends, or no ⌘-scroll for G), the camera lays the tracks out once at the new zoom and takes the stretch away in the same frame. The picture after is the same as today's, byte for byte (measured).
- The tracks are laid out early only when a zoom out shows more than the drawn margin covers, about ×1.5 out or ×2 in.

**Measured** (median work per frame, ms; the 95th percentile in brackets):

| Song, engine | Today | Z1 |
|---|---|---|
| Beat, WebKit | 38 (56) | 8 (14) |
| Big, WebKit | 73 (91) | 15 (44) |
| Beat, Chrome | 14.2 (18.9) | 1.9 (2.5) |

The one real layout at the end still costs what a frame costs today, 38 to 51 ms on the beat song in WebKit. Z2 makes that smaller.

**What you see.**
- Names, bar numbers and clip borders stretch while your fingers zoom, and snap back sharp when they stop.
- The mid-pinch pictures are on the branch (`~/m2-runs/pinch-cost/look/`).
- Maps and many web apps zoom this way. Logic and Ableton redraw on every frame instead, because their native drawing is fast enough.

**Who else must follow the stretch.** These are drawn over the tracks: the playhead, the loop's shade, sections, markers, the selection's box and the grid. They go inside the stretched layer, or follow the same numbers.

**The camera keeps two zooms during a live zoom:** the zoom shown (what the fingers asked for) and the zoom laid out.
- Everything that reads the camera from outside sees the zoom shown: `camera.now()`, agents' `seen`, Follow, and the playhead.
- The zoom laid out is the views' business.

**While the stretch lasts, the tracks don't take the pointer** (`pointer-events: none`). That saves WebKit finding what is under the fingers on each event, and hover is muted while the camera moves anyway (VIEW-8).

**Notes and the beat grid** get the same live zoom across. The Graph doesn't need it: its zoom is already cheap (9 ms on the beat song in WebKit). Instead, its nodes stop re-rendering on a zoom (§3).

## 3. Z2: cheaper redraws

- **Clips draw their notes, steps and curves on a canvas,** one per clip, redrawn only when the clip's notes change or the zoom settles.
  - Measured with a stand-in: −26% (beat) to −33% (big) per frame in WebKit, and −55% to −61% in Chrome.
  - The 2 px marks and each note's loudness (its opacity) move into the canvas drawing.
  - Tests that read the drawing's SVG read the canvas's pixels, or the drawing's list of what it drew, instead. They are just as strict.
- **A narrower margin while zooming.** The tracks draw up to 2 to 4 times the clips on screen. `content-visibility: auto` on clips measured −16% on the big song in WebKit and −32% in Chrome, and nothing on the beat song. The right margin comes from measuring after the canvas lands.
- **The Graph:** its nodes don't re-render when only the zoom changes.
- **Fewer components per zoom frame:** the grid and the take row render once per zoom, not once per track. This is small, at most 10%.

Tried and not worth it: notes as a few `<path>`s (−3 to −10%); `contain: strict` on clips (no gain); clip boxes moved by `transform` (only the boxes' share, and hundreds of layers in WebKit).

## 4. The decision for the owner

**Z1: while you pinch or ⌘-scroll the tracks, the picture stretches for a moment, then snaps sharp when your fingers stop. Yes or no?**
- **Yes:** a pinch on a beat song runs near 120 frames a second.
- **No:** we do only Z2. Then a pinch on a beat song costs about 25 ms a frame in WebKit (an estimate from the measurements): about 40 frames a second on your Mac, better than today's 20 to 25.

Z2 changes nothing you see, so it goes ahead either way.

## 5. The rules that change

| Rule | Today | After |
|---|---|---|
| VIEW-5 | A zoom lands in the frame it was asked in. | With Z1: a zoom by the hand shows in the frame it was asked in, stretched; the tracks are laid out at it when it ends. A zoom by the app (⌘=, Fit) is laid out at once, as today. |
| VIEW-8 | No React component renders on a frame the camera moves, except at a step of the margin. | Also: no React component renders on a frame of a live zoom. |
| VIEW-12 | Clips draw their notes as SVG; the 2 px marks by `data-fine`. | Clips draw their notes on a canvas, redrawn when their notes change or the zoom settles. |

## 6. Build order

1. **`zoom-bench`.** The bench joins the repo as a nightly-tier measure, so later changes can be compared: `e2e/zoom-bench.mjs`, Chrome only, plus the WebKitGTK driver as a tool. It also adds a step to the swipe probe that replays a pinch and reports the per-frame split, so the owner's Mac can be measured one for one.
2. **`canvas-clips` (Z2).** Clips' canvas drawings, the Graph's nodes kept still on a zoom, the grid once per zoom. Then the margin is measured again.
3. **`live-zoom` (Z1),** if the owner says yes. It lives in the camera library: a view marks what stretches, and the camera keeps the zoom shown beside the zoom laid out.

Each track has a builder, a skeptic's review and the full check, as the camera's did.
