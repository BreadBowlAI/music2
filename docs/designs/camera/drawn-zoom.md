# The drawn zoom: a pinch as sharp as Maps

| | |
|---|---|
| **Status** | Accepted, 2026-11-14, by the lead: the owner leaves look-and-feel calls to the lead. It replaces Z1, the live zoom of [zoom.md](zoom.md). The arrangement comes first, then Notes and the beat grid (§5). |
| **Written** | 2026-11-14, from `main` at `d06bb743`, after the owner tried Z1 on their Mac: "I hate the pinch to zoom on the timeline, ew ew ew. It should be like apple maps smooth". |
| **Measured on** | Branch `maps-zoom` (a prototype, not for merge, at `adea9ca3`), in Chrome and WebKitGTK on a server with no graphics chip. The branch `maps-try` is the same with the picture on, to try on the owner's Mac. |
| **Beside it** | [design.md](design.md), the camera note, and [zoom.md](zoom.md), Z1 and Z2. This note changes VIEW-5 and VIEW-8, and adds VIEW-13. |

**In short**
- **Why Z1 is wrong:** it stretches the old picture while the fingers move, then snaps sharp when they stop. The owner sees stretched text, then a jump. Maps does neither: it draws every frame sharp.
- **The drawn zoom:** while the hand zooms, the view is drawn into a canvas laid over it. Every frame is sharp at the zoom shown, drawn from the song itself. The page under the canvas doesn't move. When the fingers stop, the page is laid out once, and the canvas hides in that same frame.
- **Nothing changes when the fingers stop.** The prototype's last picture and the page after it differ only by anti-aliasing: in Chrome, 0 to 3 pixels of 2.5 million differ by more than 32 of 255. Z1 changes 40,000 to 250,000 pixels at that moment.
- **Text matches.** Canvas 2D draws text with the page's own font engine: the same glyphs in the same places.
- **Speed:** the picture's own script costs 1.5 to 4 ms a frame. The rest is the browser turning the canvas into pixels. On a Mac, WebKit does that in its GPU process, not on the page's thread. Whether Canvas 2D reaches 120 frames a second there is measured on the owner's Mac (§6). If it doesn't, the lanes are drawn with WebGL.

## 1. What the prototype measured

The bench sends a pinch or a run of ⌘-scroll through the camera's real listeners (zoom.md §1). Each cell gives the frame's work in ms (median / 95th percentile). "Z1" is `main` at `d06bb743`, with Z2. Both browsers draw in software here, so these are not the Mac's numbers.

**WebKitGTK 2.52**

| Song, gesture | Z1: stretched frames | Z1: layouts mid-gesture, count × ms | Canvas 2D picture | WebGL picture |
|---|---|---|---|---|
| Beat song, pinch | 6 / 15 | 1 × 39 | 25 / 49 | 13 / 28 |
| Beat song, ⌘-scroll | 6 / 12 | 1.8 × 37 | 25 / 39 | 13 / 24 |
| Big song, pinch | 19 / 81 | 1.2 × 96 | 27 / 50 | 17 / 25 |
| Big song, ⌘-scroll | 20 / 67 | 2.2 × 89 | 28 / 36 | 17 / 23 |

**Chrome 133**

| Song, gesture | Z1: stretched | Z1: layouts mid-gesture | Canvas 2D | WebGL |
|---|---|---|---|---|
| Beat song, pinch | 3.0 / 13.7 | 1 × 30 | 9.3 / 15.1 | (software WebGL: not a measure) |
| Big song, pinch | 8.8 / 30.8 | 2.7 × 61 | 14.3 / 23.1 | (the same) |

- Z1's stretched frames are cheap because they draw nothing new. Its cost is the layouts in the middle of a gesture, which the drawn zoom never does.
- A blank canvas of the same size costs 4 to 12 ms here. So most of a 2D frame is software raster, which a Mac does elsewhere.
- One canvas the size of the view beat redrawing one canvas per lane (WebKit beat song: 25 against 29 ms).

**The end** (the pinch's last frame, fingers still on, against the first frame after; pixels more than 32 of 255 apart):

| Pinch | Z1 | Canvas 2D picture |
|---|---|---|
| Chrome, out to 0.65 | 39,570 | 0 |
| Chrome, in ×4 | 249,129 | 3 |
| WebKit, out | 36,776 | 7 to 10 |
| WebKit, in | 91,109 | 250 to 290 |

The one layout at the end costs about what one of Z1's layouts does (beat song: 28 ms in Chrome, 51 in WebKit). Nothing moves while it runs: the picture stays on screen until the page is drawn, and the page drawn is the same picture.

## 2. The decisions

1. **A picture, not a stretch.**
   - The camera library owns the picture's life. It shows the canvas when a zoom by the hand begins, asks the view to paint at each move of the camera, and hides the canvas in the same task that lays the view out at the end.
   - The view owns what is drawn. It gives a scene, read once as the zoom begins from what the view renders from, and a paint function of the zoom shown.
   - While the picture shows, nothing on the page is stretched, laid out early or given a layer of its own.
2. **Canvas 2D**, one canvas the size of the view, plus one for the ruler. It shares the paint code of Z2's lane canvases (`paintNotes`, `paintCurve`, `spanBox`), so notes land on the same device pixels. WebGL draws the lanes only if the owner's Mac misses 120 frames a second with 2D. Text is always drawn with 2D: WebGL's glyph atlas blurs it.
3. **The picture draws everything the arrangement can show.** It never falls back to a stretch. That covers clips, notes, curves, audio waves, lanes of names, takes (also while recording), hidden and faded rows, bus words, hatching, tempo and meter changes, the grid, the ruler, markers, the loop, the selected bars, picked clips, the playhead and the Song row. If it misses something, VIEW-13's check fails.
4. **The page under the picture stays as it was laid out.** The scroll bar is the exception, if it is cheap: the scroll room and position follow the zoom shown under the opaque picture, so the scroll bar's thumb doesn't jump at the end. If that costs too much, the thumb jumps, as with Z1, and that goes into gaps.md.
5. **A press while the picture shows ends the zoom and does nothing else,** as a touch stops a glide on an iPhone. The picture shows with no fingers on only for G (50 ms) after the last ⌘-scroll, and during a glide.
6. **One layout at the end.** Its cost on a big song is the open "laid-out frame cost" (HANDOFF.md). It can be felt only as a moving playhead stopping for a frame, or a swipe started at once waiting for it.
7. **No glide yet.** The owner's logged pinch slows nearly to a stop before the fingers lift, so a glide would rarely start. First the swipe probe gets a step "pinch and let go" to measure how fast fingers leave. Then a glide, if wanted (the prototype has one: the zoom's speed kept at 0.996 a ms).
8. **No switch.** The prototype's palette switch was for measuring. The build has the picture only.
9. **Then Notes and the beat grid,** through the same library. After that, Z1's stretch goes from the camera library (`useStretch`, the kept layers, early layout), which makes it smaller.

**Why, in one line each:**
- The owner judges motion against Maps: "sharp every frame, nothing changes when the fingers stop, 120 Hz". A shortcut that shows on screen is not acceptable, however good its numbers.
- Z1's note said "Apps on the Mac zoom this way: Maps, Preview, Safari's pinch". Safari and Preview do stretch, then sharpen. Maps doesn't, and Maps is the bar.

## 3. The rules that change

| Rule | Today (Z1) | After |
|---|---|---|
| VIEW-5 | A zoom by the hand stretches what the view marks, and lays the view out at its end, or early near the edge of what is drawn. A press lays the zoom out first and lands on what shows. | In a view that draws its zoom, a zoom by the hand shows in the frame it was asked in, drawn sharp at the zoom shown. The page under it is laid out only at the end. A press while the picture shows ends the zoom and does nothing else. Notes and the beat grid keep Z1 until §5's second track. |
| VIEW-8 | No React component renders on a frame of a live zoom. | The same for a drawn zoom: the picture paints from the camera's subscription. |
| VIEW-13 (new) | – | The last frame of a drawn zoom and the page laid out after it MUST look the same, within anti-aliasing. Nothing on screen changes when the fingers stop. |

## 4. Tests

- **The picture says what it drew.** As Z2's lane canvases have their `drawn` lists, the picture keeps a list for each frame: each clip's box at the zoom shown, the playhead, the bars and the marks.
- **The camera suite's live-zoom checks of the arrangement are rewritten, not weakened.** Each frame shows each clip within a px of the zoom shown; no render mid-zoom; the beat under the fingers holds; never laid out before the end; the end equals a direct zoom to the same place.
- **VIEW-13's check:** a pixel diff of the last picture against the page after it, on a song with every kind of thing the picture draws.
- **At a real 2 device px a px.** Chrome's `--force-device-scale-factor=2`, not puppeteer's emulated `deviceScaleFactor`: the emulation snaps boxes to whole CSS px, so the picture and the page differed by 27,000 pixels under it and by 0 at a real 2. The camera suite's `open({dpr})` moves to the real flag, so Z2's checks at 2 device px see what a Mac sees.

## 5. Build order

1. **`drawn-zoom`:** the library's part, the arrangement drawn whole, the rules and the tests. WebGL for the lanes goes in this track only if the owner's Mac asks for it by then.
2. **`drawn-zoom-2`:** Notes and the beat grid drawn the same way, and Z1's stretch removed.
3. **The glide,** after the probe measures release speeds.

Each track has a builder, a skeptic's review and the full check.

## 6. What only the owner's Mac can say

- Whether WebKit's GPU process draws a 2D frame (about 3,500 shapes in a 2,400 × 1,650 canvas) in time for 120 frames a second.
- Canvas text against the page's text in WKWebView.
- What the one layout at the end really costs.
- How fast the owner's fingers leave a pinch.

To measure it, the owner builds `maps-try`, then for each way of drawing (⇧⌘P ▸ Zoom drawing): ⇧⌘P ▸ Swipe probe ▸ the last step, Replay a pinch ▸ Copy report. The report names the way it drew. Read "between frames": 8.3 ms is 120 a second.

**Known from the prototype, to fix in the build:**
- In WebKit, far zoomed out, Z2's lane canvas draws notes up to 1 device px right of where the picture puts them. The likely cause is a lane canvas whose width ends on a half px; not proven.
- `reveal()` reads the page's places with the stretch's maths.
- The scene isn't read again if the song changes mid-zoom.
- The page's playhead shows through the rows' borders under the headers mid-zoom (Z1 too).
