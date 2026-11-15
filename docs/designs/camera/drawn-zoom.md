# The drawn zoom: a pinch as sharp as Maps

| | |
|---|---|
| **Status** | Accepted, 2026-11-14, by the lead: the owner leaves look-and-feel calls to the lead. It replaces Z1, the live zoom of [zoom.md](zoom.md). The arrangement comes first, then Notes and the beat grid (§5). **Built** by track `drawn-zoom` (§1, "Measured after the build"): the arrangement draws its zoom, Notes and the beat grid are laid out each frame, and Z1 went. |
| **Written** | 2026-11-14, from `main` at `d06bb743`, after the owner tried Z1 on their Mac: "I hate the pinch to zoom on the timeline, ew ew ew. It should be like apple maps smooth". |
| **Measured on** | Branch `maps-zoom` (a prototype, not for merge, at `adea9ca3`), in Chrome and WebKitGTK on a server with no graphics chip. The branch `maps-try` is the same with the picture on, to try on the owner's Mac. |
| **Beside it** | [design.md](design.md), the camera note, and [zoom.md](zoom.md), Z1 and Z2. This note changes VIEW-5 and VIEW-8, and adds VIEW-13. |

**In short**
- **Why Z1 is wrong:** it stretches the old picture while the fingers move, then snaps sharp when they stop. The owner sees stretched text, then a jump. Maps does neither: it draws every frame sharp.
- **The drawn zoom:** while the hand zooms, the view is drawn into a canvas laid over it. Every frame is sharp at the zoom shown, drawn from the song itself. The page under the canvas doesn't move. When the fingers stop, the page is laid out once, and the canvas hides in that same frame.
- **Nothing changes when the fingers stop.** The prototype's last picture and the page after it differ only by anti-aliasing: in Chrome, 0 to 3 pixels of 2.5 million differ by more than 32 of 255. Z1 changes 40,000 to 250,000 pixels at that moment.
- **Text matches.** Canvas 2D draws text with the page's own font engine: the same glyphs in the same places.
- **Speed:** the picture's own script costs 1.5 to 4 ms a frame. The rest is the browser turning the canvas into pixels. On a Mac, WebKit does that in its GPU process, not on the page's thread. On the owner's Mac it does: a drawn frame's work is 2 ms, and none was over 8.3 ms (§6, "Answered by the owner's Mac"). So no WebGL.

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

### Measured after the build

Track `drawn-zoom` against main at `d06bb743` (Z1), on this server (Linux, no GPU): `zoom-bench` in Chrome 133 and the WebKit tool (`e2e/webkit/`, WebKitGTK 2.50), each at 2 device px a px, three runs of each gesture, one at a time. "Pinch out" is the probe's A13 way out alone (to a third), which only the end lays out at a new zoom. Each cell is median / 95th percentile in ms, frames counted in brackets. A run's layout: how many frames a run laid out, then that frame's work. Pictures (mid-pinch, the last frame, the frame after, the diff): `~/m2-runs/drawn-zoom/look/`.

**WebKitGTK**

| Song, gesture | Z1: a stretched frame's work | Z1: layouts a run × their work | Z1: between frames | Drawn: a frame's work | Drawn: layouts a run × their work | Drawn: between frames |
|---|---|---|---|---|---|---|
| Beat song, pinch | 6 / 10 (124) | 1 × 38 / 64 | 18 / 26 | 20 / 27 (95) | 0 | 26 / 32 |
| Beat song, ⌘-scroll | 6 / 11 (147) | 1.7 × 32 / 78 | 18 / 29 | 21 / 27 (115) | 0.7 × 27 | 27 / 32 |
| Beat song, pinch out | 6 / 13 (43) | 2.3 × 38 / 41 | 19 / 70 | 21 / 27 (51) | 1 × 40 / 74 | 25 / 32 |
| Big song, pinch | 20 / 28 (67) | 1 × 90 / 113 | 26 / 97 | 25 / 33 (84) | 0 | 29 / 37 |
| Big song, ⌘-scroll | 20 / 35 (59) | 2.7 × 95 / 119 | 29 / 136 | 25 / 30 (104) | 0.7 × 53 | 29 / 36 |
| Big song, pinch out | 15 / 33 (16) | 2.7 × 71 / 91 | 37 / 135 | 25 / 31 (47) | 1 × 130 | 29 / 37 |

**Chrome 133**

| Song, gesture | Z1: a stretched frame's work | Z1: layouts a run × their work | Z1: between frames | Drawn: a frame's work | Drawn: layouts a run × their work | Drawn: between frames |
|---|---|---|---|---|---|---|
| Beat song, pinch | 2.7 / 5.1 (154) | 1 × 29 / 33 | 16.7 / 16.7 | 7.9 / 10.6 (158) | 0 | 16.7 / 16.8 |
| Beat song, ⌘-scroll | 2.4 / 3.3 (173) | 2 × 27 / 28 | 16.7 / 16.8 | 8.1 / 11.0 (180) | 0.7 × 24 | 16.7 / 16.8 |
| Beat song, pinch out | 2.6 / 10.3 (64) | 3 × 25 / 35 | 16.7 / 16.7 | 8.0 / 10.4 (78) | 1 × 29 / 31 | 16.7 / 16.8 |
| Big song, pinch | 7.5 / 19.8 (135) | 3 × 59 / 91 | 16.7 / 33.3 | 11.1 / 15.4 (158) | 0 | 16.7 / 16.7 |
| Big song, ⌘-scroll | 8.1 / 11.2 (147) | 3.3 × 61 / 95 | 16.7 / 33.3 | 11.7 / 14.3 (180) | 0.7 × 58 | 16.7 / 16.7 |
| Big song, pinch out | 7.3 / 26.4 (50) | 3 × 41 / 61 | 16.7 / 33.4 | 11.7 / 14.6 (78) | 1 × 107 / 115 | 16.7 / 16.7 |

- The bench's pinch and ⌘-scroll end at the zoom they began at (the probe's A13 in and back out), so their end lays out little or nothing new (⌘-scroll's 0.7: one layout in two of three runs). The pinch out's end is the one layout of a zoom: 29 ms (beat) and 107 ms (big) in Chrome, 40 and 130 in WebKit. Z1 laid out in the middle of a gesture 1 to 3.3 times a run, at about the same cost each.
- A drawn frame costs more than a stretched one (it paints the view: in Chrome 8 against 2.7 ms on the beat song), but never lays out, so its 95th percentile is flat: the frames between stay at Chrome's 16.7 ms (its headless frame) on the big song, where Z1 dropped to 33. In WebKit here a drawn frame is 20 to 25 ms: most of it is software raster on this server (a blank canvas the size of the view costs 4 to 12 ms, §1 above); a Mac rasters on its GPU. Only the owner's Mac can say if it makes 120 frames a second (§6).
- **The scroll room tried, not kept** (§2.4): following the zoom shown under the picture made a Chrome drawn frame 7.9 → 9.5 ms on the beat song and 11.1 → 17.3 ms on the big song (each frame lays the content's width out again), and 20 → 35 and 25 → 51 ms in WebKit.

**The end** (VIEW-13: the last frame, fingers still on, against the page laid out after it; pixels more than 32 of 255 apart, the arrangement's clear part):

| Zoom | Chrome, the `drawn-zoom` suite's song, 3.95 million px | Chrome, beat song, by hand (`look.mjs`), 2.5 million px | WebKitGTK, beat song, by hand (`wklook`), 6.3 million px | WebKitGTK, big song, by hand |
|---|---|---|---|---|
| Out to the song's start and back in | 0 | – | – | – |
| In | 53 | 288 | 266 | 160 |
| Out | 112 | 428 | 7 | 41 |
| Far out | 135 | 846 | 4,058 | 4,395 |
| ⌘-scroll | 30 | 738 | – | – |

- Chrome's are the same in each of three runs: text a sub-pixel apart (marker names, a lane of names' steps), where Chrome places the page's glyphs on its own grid. The suite's limit is 200.
- On the beat song in Chrome, every pixel over 32 apart is in the ruler's bar numbers (the most 67 of 255): where a number's left falls on a fraction of a device px, the picture's glyphs sit up to half a device px left of the page's. Both are placed at the same px (within a hundredth of a device px); how Chrome rasters the page's text there is what differs, and on the suite's song it doesn't. The cause isn't found (gaps.md); WebKit draws them alike.
- **After review T**, the suite also compares with the playhead in view, a lane glowing (AGENT-5) with its clip picked, the pointer resting on a clip with the Mac's hover rules on, a clip far on whose peaks the page never asked for, and a pinch while a take records and the song plays (the columns the playhead crossed between the two shots left out). Its eight zooms: 0, 40, 87, 31, 87, 91, 30 and 0 pixels over 32 apart.

**While the song plays** (review T4), the bench's timeline pinch (A13 in and back), three runs stopped and three playing, Chrome, at `28f96595` (the picture painted on the clock's frames and again in the camera's: about twice a frame) and after (once a frame, in the camera's frame). A drawn frame's work, median / 95th percentile in ms:

| Song | Stopped | Playing, before | Playing, after |
|---|---|---|---|
| Beat song | 7.5 / 10.7 | 12.0 / 16.8 | 8.2 / 10.8 |
| Big song | 11.1 / 13.7 | 18.7 / 28.6 | 13.1 / 20.7 |

On the beat song the frames between stay at 16.7 ms playing (33.2 at the 95th percentile before). What is left on the big song is the page's own while it plays (its playhead and meters). Playing, a run laid out 1.7 to 2.3 times (stopped: none): most likely Follow paging to the playhead, which lays a drawn zoom out first (VIEW-5); not looked into further.

**What VIEW-13's check sees.** Beside the pixels over 32 apart (at most 200), the mean difference of each 8 × 8 device px cell: at most 8 of 255, or 20 where the picture lists a word (glyphs fall a sub-pixel apart in the page's text, and only there). Measured on the suite's song: at most 3.8, and 12.8 where a word lies. A faint wash moved or a small shape left out differs by under 32 in each pixel but is dense in its cells. Each of review T's mutants, built from this source, against the suite's eight zooms (pixels over 32 / cells over the limit, the zooms that fail):

| Mutant | Fails | The most apart |
|---|---|---|
| No strike-through on a name the choice lost | 5 of 8 | words' cells 28 |
| No playhead cap | 5 of 8 | cells 115 |
| The loop's shade drawn a beat right (and `camera` fails: the shade 24 px off) | 6 of 8 | 2,109 cells over, each 16 |
| No curve points | 7 of 8 | cells 44 |
| No step ticks in the lane of names | 5 of 8 | cells 36 |
| No fade handles on the picked audio clip | 4 of 8 | cells 83, words' 105 |
| No hatching on the track muted in the song | 7 of 8 | cells 9.3 to 12 |
| No marker ticks | 8 of 8 | cells 28 |
| No tempo or meter words | 8 of 8 | cells 74 |
| No audio waves | 7 of 8 | 19,975 px |
| The picture 1 px across | 8 of 8 | 232,355 px |

The hatching is the closest: 9.3 against the limit's 8 and the song's own 3.8.

- WebKit's are compared by hand: the WebKit tool drives no suite. Nearly all its pixels over 32 are WebKitGTK's overlay scroll bars, which show after the end's scroll write (the vertical one: 266 of the beat song's 266 in; the horizontal one: 4,006 of its 4,058 far out). The rest differ by under 32 (grid lines' anti-aliasing). Before the lane canvas lay on whole px, the beat song's far out was 16,895 (§6).

## 2. The decisions

1. **A picture, not a stretch.**
   - The camera library owns the picture's life. It shows the canvas when a zoom by the hand begins, asks the view to paint at each move of the camera, and hides the canvas in the same task that lays the view out at the end.
   - The view owns what is drawn. It gives a scene, read once as the zoom begins from what the view renders from, and a paint function of the zoom shown.
   - While the picture shows, nothing on the page is stretched, laid out early or given a layer of its own.
2. **Canvas 2D**, one canvas the size of the view, plus one for the ruler. It shares the paint code of Z2's lane canvases (`paintNotes`, `paintCurve`, `spanBox`), so notes land on the same device pixels. WebGL draws the lanes only if the owner's Mac misses 120 frames a second with 2D. *Measured: it doesn't* (§6). Text is always drawn with 2D: WebGL's glyph atlas blurs it.
3. **The picture draws everything the arrangement can show.** It never falls back to a stretch. That covers clips, notes, curves, audio waves, lanes of names, takes (also while recording), hidden and faded rows, bus words, hatching, tempo and meter changes, the grid, the ruler, markers, the loop, the selected bars, picked clips, the playhead and the Song row. *Built:* VIEW-13's check (`drawn-zoom`) fails a picture that leaves any one of those out, of the kinds tried: each of the review's mutants fails at least one of its eight zooms (§1, "What VIEW-13's check sees"). It can't see a kind its song doesn't show: a take of notes recording (it records audio), a lane just made, lit for a moment (not drawn, gaps.md).
4. **The page under the picture stays as it was laid out.** The scroll bar was to be the exception, if it was cheap: the scroll room and position following the zoom shown under the picture, so the scroll bar's thumb doesn't jump at the end. *Built: not kept.* The room has to lay the page's content out at a new width each frame: in Chrome a drawn frame went from 7.9 to 9.5 ms on the beat song and from 11.1 to 17.3 ms on the big song (§1, "Measured after the build"). So the thumb jumps at the end, as with Z1 (gaps.md).
5. **A press while the picture shows ends the zoom and does nothing else,** as a touch stops a glide on an iPhone. The picture shows with no fingers on only for G (50 ms) after the last ⌘-scroll, and during a glide.
6. **One layout at the end.** Its cost on a big song is the open "laid-out frame cost" (HANDOFF.md). It can be felt only as a moving playhead stopping for a frame, or a swipe started at once waiting for it.
7. **No glide yet.** The owner's logged pinch slows nearly to a stop before the fingers lift, so a glide would rarely start. First the swipe probe gets a step "pinch and let go" to measure how fast fingers leave. Then a glide, if wanted (the prototype has one: the zoom's speed kept at 0.996 a ms).
8. **No switch.** The prototype's palette switch was for measuring. The build has the picture only.
9. **No stretch anywhere** (the owner, of Notes and the beat grid too: "I don't like how the timeline stretches when I'm trying to pinch in and out. It should be like updating in real time, just using best practices"). Notes and the beat grid are laid out at the zoom shown each frame, as the Graph is, until the second track draws them as pictures through the same library. Z1's code went with the first track (`useStretch`, `stretchOf`, `shownAt`, `shownScroll`, the kept layers, the early layout): the camera library is smaller, and keeps `laid` for the drawn zoom alone.

**Why, in one line each:**
- The owner judges motion against Maps: "sharp every frame, nothing changes when the fingers stop, 120 Hz". A shortcut that shows on screen is not acceptable, however good its numbers.
- Z1's note said "Apps on the Mac zoom this way: Maps, Preview, Safari's pinch". Safari and Preview do stretch, then sharpen. Maps doesn't, and Maps is the bar.

## 3. The rules that change

| Rule | Today (Z1) | After |
|---|---|---|
| VIEW-5 | A zoom by the hand stretches what the view marks, and lays the view out at its end, or early near the edge of what is drawn. A press lays the zoom out first and lands on what shows. | In a view that draws its zoom, a zoom by the hand shows in the frame it was asked in, drawn sharp at the zoom shown. The page under it is laid out only at the end. A press while the picture shows ends the zoom and does nothing else. A view that doesn't draw its zoom yet (Notes, the beat grid, the Graph) is laid out at the zoom shown each frame. Nothing stretches. A run of ⌘-scroll ends after G counted in frames, each at most G/2, never by a timer. |
| VIEW-8 | No React component renders on a frame of a live zoom. | The same for a drawn zoom: the picture paints from the camera's subscription. |
| VIEW-13 (new) | – | The last frame of a drawn zoom and the page laid out after it MUST look the same, within anti-aliasing. Nothing on screen changes when the fingers stop. |

## 4. Tests

- **The picture says what it drew.** As Z2's lane canvases have their `drawn` lists, the picture keeps a list for each frame: each clip's box at the zoom shown, the playhead, the bars and the marks.
- **The camera suite's live-zoom checks of the arrangement are rewritten, not weakened.** Each frame shows each clip within a px of the zoom shown; no render mid-zoom; the beat under the fingers holds; never laid out before the end; the end equals a direct zoom to the same place.
- **VIEW-13's check:** a pixel diff of the last picture against the page after it, on a song with every kind of thing the picture draws.
- **At a real 2 device px a px.** Chrome's `--force-device-scale-factor=2`, not puppeteer's emulated `deviceScaleFactor`: the emulation snaps boxes to whole CSS px, so the picture and the page differed by 27,000 pixels under it and by 0 at a real 2. The camera suite's `open({dpr})` moves to the real flag, so Z2's checks at 2 device px see what a Mac sees.

## 5. Build order

1. **`drawn-zoom`:** the library's part, the arrangement drawn whole, the rules and the tests; Notes and the beat grid laid out each frame, and Z1's stretch removed (§2.9). WebGL for the lanes goes in this track only if the owner's Mac asks for it by then.
2. **`drawn-zoom-2`:** Notes and the beat grid drawn the same way.
3. **The glide,** after the probe measures release speeds.

Each track has a builder, a skeptic's review and the full check.

## 6. What only the owner's Mac can say

- Whether WebKit's GPU process draws a 2D frame (about 3,500 shapes in a 2,400 × 1,650 canvas) in time for 120 frames a second.
- Canvas text against the page's text in WKWebView.
- What the one layout at the end really costs.
- How fast the owner's fingers leave a pinch.

To measure it, the owner builds `maps-try`, then for each way of drawing (⇧⌘P ▸ Zoom drawing): ⇧⌘P ▸ Swipe probe ▸ the last step, Replay a pinch ▸ Copy report. The report names the way it drew. Read "between frames": 8.3 ms is 120 a second.

**Answered by the owner's Mac, 2026-11-15.** A MacBook Pro with ProMotion, the app built from `main` at `c8f99c06`, the song Beat Pack (10 tracks), the swipe probe's report:
- **120 frames a second: yes, with Canvas 2D.** The page ran at 125 frames a second (8 ms between frames, the median). The replayed pinch drew 106 frames over the timeline. A frame's work: 2 ms median, 3 ms at the 95th percentile, 6 ms the slowest. None was over 8.3 ms. So the lanes stay Canvas 2D (§2.2).
- **The one layout at the end:** 23 ms (the replay's way out alone), and 1 ms for the frame after it. Nothing moves on that frame, since the last picture equals the page (VIEW-13). Only a moving playhead, or a swipe started at once, would be about 2 frames late.
- **The picture painted 2,080 times and never failed:** no fallback to a layout each frame.
- **How fast fingers leave a pinch** (step 15, asked to let go): 6 of 9 left quickly, at 4.0 to 8.4 a second (the scale's ln). Their last 100 ms zoomed ×0.52 to ×0.66 out, or ×1.57 in once. The other 3 left slowly, at 0.02 to 1.9. The last change came 4 to 8 ms before the fingers lifted. The pinches of the other steps slowed nearly to a stop before the lift. The glide starts from these numbers (§5).
- **Canvas text against the page's text:** for the owner's eyes; the report can't say.

**Known from the prototype, fixed in the build** (each with a check that failed first):
- In WebKit, far zoomed out, Z2's lane canvas drew notes 1 device px right of where the picture puts them. The cause: WebKit draws a canvas into its box rounded to whole CSS px, and a lane's canvas lay on a half px (on the screen's own pixels, not whole px). A lane's canvas lies on whole px now (`ClipDrawing.tsx`). WebKit, beat song, a pinch far out: 16,895 pixels over 32 apart at the end, 4,058 after (§1).
- `reveal()` read the page's places with the stretch's maths. Under the picture it carries the page's place to the zoom shown (`rig.ts`).
- The scene wasn't read again if the song changed mid-zoom. Each render of the arrangement and of a track's rows says what it draws (`Said`), and the stores the scene reads are heard: the picture reads its scene again.
- The page's playhead showed through the lines between the tracks' headers, mid-zoom and at rest when scrolled past. The page's playhead keeps to the zoom laid out, and hides when scrolled past.

**Found in the build, fixed:**
- A run of ⌘-scroll ended by a timer, which a long frame could fire before the events queued behind it (main's CI). It ends by frames now.
- A zoom back at the zoom it began at, its way out stopped by the view's start, looked further on than the view had room for: its end jumped back. It asks for its room as any zoom's end does.
