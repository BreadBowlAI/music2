# Zooming at the screen's rate

| | |
|---|---|
| **Status** | Accepted, 2026-11-14: Z1 and Z2 (§4). Z2 built (track `canvas-clips`): §3 says what changed and what it measured. Z1 built by track `live-zoom`, on top of Z2 ("Measured after Z1", §2). |
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
- The tracks are laid out early only when a zoom out comes near the edge of what is drawn: a step before it, so the new clips come between frames. A zoom in is laid out only at its end. *(As built; the note first said "about ×1.5 out or ×2 in".)*

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

**While the stretch lasts, the tracks still take the pointer.** *(As built; the note first said `pointer-events: none`.)* The browser hits the picture as it shows, so a press lands on what you see, and lays the zoom out first. Hover is muted while the camera moves anyway (VIEW-8). Measured, `pointer-events: none` cost more than it saved: see "Departures from this note" below.

**Notes and the beat grid** get the same live zoom across. The Graph doesn't need it: its zoom is already cheap (9 ms on the beat song in WebKit). Instead, its nodes stop re-rendering on a zoom (§3).

### Measured after Z1

Track `live-zoom` built Z1. Today is `main` at `6d4963eb`; Z1 is the track's branch. Both were measured in turn on one machine, with the WebKit tool (WebKitGTK 2.52, drawing in software) and `zoom-bench` (headless Chrome 133), at 1440×900 and 2 device px a px. Each gesture ran three times over the timeline, from 24 px a beat: the owner's A13 pinch (out to 0.44 and back) and a run of ⌘-scroll (out and back). "Work" is a frame's main-thread time, as the zoom bench splits it.

Work per frame while the gesture lasts, in ms: median (95th percentile); then the frames over 8.3 ms, of all the frames drawn:

| Song, engine, gesture | Today | Z1 |
|---|---|---|
| Beat, WebKit, pinch | 37 (51); 41 of 46 | 9 (46); 61 of 111 |
| Beat, WebKit, ⌘-scroll | 40 (55); 45 of 48 | 8 (46); 55 of 122 |
| Big, WebKit, pinch | 94 (115); 19 of 21 | 16 (89); 89 of 93 |
| Big, WebKit, ⌘-scroll | 99 (110); 18 of 21 | 23 (85); 89 of 96 |
| Beat, Chrome, pinch | 22 (36); 138 of 152 | 2.3 (17); 10 of 168 |
| Beat, Chrome, ⌘-scroll | 23 (37); 157 of 168 | 2.2 (3.5); 3 of 179 |
| Big, Chrome, pinch | 43 (62); 338 of 340 | 10 (33); 101 of 155 |
| Big, Chrome, ⌘-scroll | 63 (76); 353 of 354 | 12 (21); 115 of 166 |

Z1's frames apart, in ms: median / 95th percentile, from the first event to 200 ms after the last. A **stretched** frame is only a transform apart from the frame before. A **laid-out** frame draws a new zoom: an early one, as a zoom out nears the edge of what is drawn (A13 goes out to 0.44, past it), or the one at the end. Then the frame after each.

| Song, engine, gesture | stretched frames | their work | laid out, a run | a laid-out frame | the frame after it | Today's frames |
|---|---|---|---|---|---|---|
| Beat, WebKit, pinch | 94 | 9 / 14 | 3 | 47 / 71 | 10 / 44 | 37 / 51 |
| Beat, WebKit, ⌘-scroll | 106 | 8 / 12 | 4 | 47 / 80 | 11 / 21 | 40 / 55 |
| Big, WebKit, pinch | 74 | 16 / 49 | 3 | 89 / 121 | 28 / 40 | 94 / 115 |
| Big, WebKit, ⌘-scroll | 71 | 20 / 46 | 7.3 | 81 / 99 | 77 / 92 | 99 / 110 |
| Beat, Chrome, pinch | 147 | 2.3 / 3.7 | 3 | 19 / 20 | 2.4 / 8.4 | 22 / 36 |
| Beat, Chrome, ⌘-scroll | 174 | 2.2 / 3.3 | 2 | 22 / 23 | 2.2 / 2.8 | 23 / 37 |
| Big, Chrome, pinch | 138 | 10 / 15 | 3 | 38 / 61 | 9 / 10 | 43 / 62 |
| Big, Chrome, ⌘-scroll | 159 | 11 / 20 | 2 | 73 / 77 | 18 / 18 | 63 / 76 |

These were measured at `bb1e472b`. The fixes after the review (Notes and the beat grid given room where a zoom looked, a zoom started from the box's whole px, the browser's move taken in) change nothing these runs measure in the timeline.

- **A pinch frame on a beat song in WebKit went from 37 ms to 9**, as the prototype said (38 to 8). WebKit here drew 111 frames in the time it drew 46. On the big song, 94 to 16.
- **A laid-out frame costs about what a frame cost before**, 47 ms on the beat song in WebKit, and the frame after it is cheap. Z2 makes the laid-out ones cheaper.
- **The first frame of a zoom** puts what stretches on layers of its own: about 20 ms on the beat song in WebKit.

**Departures from this note**, each measured. The lead kept all three.

1. **The stretched layers take the pointer.** The note said `pointer-events: none`. It is inherited, so WebKit works out every clip's style anew at both ends of a zoom, and it found nothing faster. One element over the view to take the pointer instead was worse: WebKit composites it over the layers. The A/B, in WebKit on the beat song (pinch, ms, median / 95th percentile; two runs each, in turn):

   | Built | the laid-out frame | the frame after it | the first frame | input, a stretched frame |
   |---|---|---|---|---|
   | Z1 | 53 / 73 | 9 / 45 | 25 / 40 | 4 |
   | with `pointer-events: none` on what is stretched | 84 / 103 | 59 / 70 | 61 / 75 | 4 |
   | with one element over the view taking the pointer | 99 / 138 | 91 / 111 | 80 / 101 | – |

   On the big song the laid-out frame was 203 ms with `pointer-events: none`, against 90 without. So a press lands on what the stretch shows, and lays the zoom out first.
2. **A zoom out is laid out a step before the edge of what is drawn**, where the view would ask for a new window anyway (VIEW-12), not at the edge. Then the view draws its new clips between frames. Laid out only once the edge was reached, it drew them all in one frame: 150 to 220 ms on the big song in Chrome. A zoom in is laid out only at its end.
3. **The layers stay a moment after a zoom**, with a transform that moves nothing, until the cameras have been still for 400 ms (`LAYERS_KEPT`). WebKit lays an element out again when its transform goes. With the transforms taken away in the frame the zoom ends, the frame after it cost 31 to 34 ms instead of 9 to 11 (WebKit, beat song). Letting the layers go at rest costs one frame of 14 to 33 ms. A press or a key lets them go at once.

- **Not measured on the owner's Mac yet.** The swipe probe's last step, Replay a pinch, measures it there with the same bench, and its report splits the timeline's frames the same way: stretched, laid out, and the frame after each.
- **Pictures** for the owner, before, mid-pinch and after, on a beat song, in the arrangement and in Notes: `~/m2-runs/live-zoom/look/`. The app has one look (taste.md: no dark mode).

## 3. Z2: cheaper redraws

- **Clips draw their notes, steps and curves on a canvas,** one per clip, redrawn only when the clip's notes change or the zoom settles.
  - Measured with a stand-in: −26% (beat) to −33% (big) per frame in WebKit, and −55% to −61% in Chrome.
  - The 2 px marks and each note's loudness (its opacity) move into the canvas drawing.
  - Tests that read the drawing's SVG read the canvas's pixels, or the drawing's list of what it drew, instead. They are just as strict.
- **A narrower margin while zooming.** The tracks draw up to 2 to 4 times the clips on screen. `content-visibility: auto` on clips measured −16% on the big song in WebKit and −32% in Chrome, and nothing on the beat song. The right margin comes from measuring after the canvas lands.
- **The Graph:** its nodes don't re-render when only the zoom changes.
- **Fewer components per zoom frame:** the grid and the take row render once per zoom, not once per track. This is small, at most 10%.

Tried and not worth it: notes as a few `<path>`s (−3 to −10%); `contain: strict` on clips (no gain); clip boxes moved by `transform` (only the boxes' share, and hundreds of layers in WebKit).

### Built: what changed from the plan (track `canvas-clips`, 2026-11-14)

- **One canvas a lane, not one a clip.** Chrome makes every canvas a layer of its own, whatever it is told. One canvas a clip gave the beat song's arrangement 416 layers (main has 36), and a frame with nothing new to draw cost 7.1 ms (main 1.5): Chrome's step that sorts what it drew into layers took most of it, so a zoom was no cheaper in Chrome. One canvas a lane draws every clip of the lane that sits still, each inside its box, in the lane's order, each clearing its box first: 47 layers, and that frame 2.1 ms. A clip that moves with a drag, a copy where it lands, a clip a later one lies over, and a take while it records draw on a canvas of their own, in their box (a few at a time).
- **Notes, their 2 px marks and a curve's line and shade** are on the canvases. A curve's points and a lane of names' steps stay elements: they were never SVG, they are few, a point stays round while the drawing stretches, and a step's name is text.
- **Kept sharp.** Each canvas is drawn at the screen's own pixels, and lies on them, so nothing is resampled while it holds still. Each clip's box lies on them too: the camera places it by `spanBox` (its start and width rounded to the screen's pixels), and the lane's canvas finds the box by the same function.
- **When a canvas is drawn again:** when what it draws changes (a lane's clips are kept by value, so a new state of the song draws only the lanes whose clips changed), at most once a frame for such changes; on a zoom by the app (⌘=, Fit) in the frame it lands in; on a pan only when what shows reaches past the part it holds. On a zoom by the hand, Z2 alone drew it once the zoom settled (no zoom for G, nor in a whole frame), stretching it until then. Since Z1 (track `live-zoom`), it draws when the zoom laid out changes, in the frame that lays it out (the live zoom's end, or an early layout), and the camera's one transform stretches it with its clips' boxes until then: ClipDrawing's own detector of a zoom's end went.
- **Bounded:** a canvas holds what shows and up to a view each side (the culling's margin), never a bitmap over 8192 × 512 device px; a lane's covers only where its clips are.
- **The Graph's nodes** render no more on a zoom (5 nodes, 10 frames of a pinch: 50 renders before, 0 now). **The grid** renders once a zoom: each row draws it with shared SVG patterns that one component spaces by the zoom (before: 26 renders for each render of the arrangement on the big song). **The take** renders on a zoom only while it records (before: once a lane, 23).
- **What the review found, and what changed for it.** Two things the owner would have seen: where clips lay over each other (a drag, a MIDI file), both clips' notes showed, and an earlier automation clip's curve and points showed through the later one, which plays there; and at 2 device px a px, far zoomed out, some clips' drawings reached half a px over their border, as the canvas rounded the box's ends and the browser rounded them otherwise. Both are fixed: a clip a later one lies over draws in its own box, under that one, as on main; and one function places the box and finds it for the canvas. The camera suite now runs at 2 device px a px, as the owner's Mac draws, and checks both.
- **Nothing looks different.** Pictures of both songs at 24 px a beat match main's to within 8 of 255 in a colour channel (the big song's within 1), and zoomed in ×3.4 to within 32. Far zoomed out (×0.13), some clip boxes' edges sit one device px from where main's browser put them, as the app now puts each box on the screen's pixels itself; the notes and marks inside them follow their boxes.
- **While a zoom by the hand lasts**, Z2 alone stretched a lane's canvas around the lane's start while each box was placed by the zoom with its 1 px ends, so notes could sit up to 2 px off their boxes until the zoom settled. Z1 ended this: the camera stretches the lane, its canvas and its boxes as one.
- **With Z1** (built), the canvases draw in the frame a live zoom is laid out, as for a zoom by the app; the camera suite checks it in every frame of a live zoom (each lane's canvas drawn at the zoom laid out, and stretched once).

### Measured after Z2

The zoom bench in Chrome and the WebKit tool, three rounds each on the server, `main` at `6d4963eb` and this branch one after the other, each run waiting for a quiet machine. Median work of a frame while the gesture lasts, in ms (the 95th percentile in brackets); then the heaviest frame after it (the canvases drawn again as the zoom settles); then the whole run's work.

| Song, engine, gesture | main | Z2 | change | after: main → Z2 | whole run: main → Z2 |
|---|---|---|---|---|---|
| Beat, Chrome, pinch | 15.4 (21.6) | 8.9 (14.4) | −42% | 2.8 → 20.8 | 787 → 597 (−24%) |
| Beat, Chrome, ⌘-scroll | 15.9 (21.0) | 9.1 (14.0) | −43% | 2.7 → 20.6 | 936 → 645 (−31%) |
| Beat, Chrome, Graph pinch | 3.7 (4.9) | 3.3 (4.5) | −11% | 2.9 → 3.4 | 262 → 246 |
| Big, Chrome, pinch | 45.9 (66.4) | 30.7 (43.7) | −33% | 20.1 → 36.5 | 5735 → 3206 (−44%) |
| Big, Chrome, ⌘-scroll | 65.3 (81.1) | 36.8 (49.0) | −44% | 20.0 → 53.4 | 7891 → 4133 (−48%) |
| Big, Chrome, Graph pinch | 12.8 (17.0) | 9.1 (11.2) | −29% | 11.4 → 8.6 | 888 → 664 |
| Beat, WebKit, pinch | 40 (66) | 27 (47) | −33% | 8 → 23 | 635 → 556 (−12%) |
| Beat, WebKit, ⌘-scroll | 39 (65) | 28 (40) | −28% | 8 → 16 | 681 → 583 (−14%) |
| Beat, WebKit, Graph pinch | 15 (18) | 11 (16) | −27% | 7 → 5 | 622 → 523 |
| Big, WebKit, pinch | 86 (184) | 53.5 (88) | −38% | 85 → 43 | 803 → 649 (−19%) |
| Big, WebKit, ⌘-scroll | 92.5 (124) | 60 (88) | −35% | 86 → 32 | 1036 → 691 (−33%) |
| Big, WebKit, Graph pinch | 24 (33) | 19 (25) | −21% | 15 → 13 | 855 → 704 |

- Every zoom frame of the tracks, which today is a whole layout of them, is a third to nearly a half cheaper, in both engines, as the plan said. Most of what is left is the clip boxes and their layout: Z1's to take away while the fingers move.
- The new cost is the frame that draws the canvases again once a zoom settles: 21 ms on the beat song in Chrome (8 ms of drawing, the rest handing the bitmaps over), up to 54 ms on the big song. It comes once, after the gesture; a whole run's work still falls by 12% to 48%. The review measured it apart, in Chrome at 2 device px a px, the slowest frame after each of six hand zooms: beat song 5.6 ms on main, 10.1 here; big song 15.2 and 22.4. Most of it is the canvases' bitmaps handed over (25 ms of a 38 ms traced settle on the big song; main's 1.9). Holding a tenth of a view each side, not a view, made it 15% to 21% cheaper, and swipes draw again sooner then; drawing the lanes that show first did nothing.
- **Tried with Z1, not kept: drawing what shows first at the landing frame.** In the frame a live zoom is laid out, each lane's canvas held what shows and a tenth of a view each side, and its whole margin came in a frame once the view was still. Median work of that laid-out frame, ms, today's build against the variant, two rounds each: in Chrome it was cheaper (beat pinch 30 → 19, ⌘-scroll 28 → 25; big pinch 59 → 45, ⌘-scroll 63 → 59), but in WebKit, which the Mac runs, it was not (beat pinch 49 → 53 and 84 → 97; beat ⌘-scroll 46 → 57 and 76 → 87; big pinch 87 → 105; big ⌘-scroll 88 → 101 and 98 → 73), and it adds a second draw at rest.
- WebKit draws in software here, so its paint is dearer than on a Mac; Chrome's numbers are from its headless shell. How WebKit on the Mac handles these canvases (`willReadFrequently`, kept on the CPU, which its code paints into the layer under them) is not measured yet: the swipe probe's Replay a pinch measures it on the owner's Mac.

**The margin, measured again with the canvases in** (the same bench, two rounds; median work of a zoom frame):

| Song, engine, gesture | Z2 | with `content-visibility: auto` on clips | with half a view of margin (step a quarter) |
|---|---|---|---|
| Beat, Chrome, pinch | 9.0 | 9.7 (+8%) | 8.5 (−6%) |
| Beat, Chrome, ⌘-scroll | 9.1 | 9.6 (+5%) | 8.2 (−10%) |
| Big, Chrome, pinch | 32.1 | 30.2 (−6%) | 19.5 (−39%) |
| Big, Chrome, ⌘-scroll | 37.5 | 33.0 (−12%) | 22.8 (−39%) |
| Beat, WebKit, pinch | 28 | 29 | 29 |
| Beat, WebKit, ⌘-scroll | 28 | 29 | 29 |
| Big, WebKit, pinch | 56 | 45 (−20%) | 42.5 (−24%) |
| Big, WebKit, ⌘-scroll | 64.5 | 55 (−15%) | 51 (−21%) |

- **`content-visibility: auto` on clips: not kept.** No gain on the beat song (a little worse in Chrome), a small one on the big song.
- **Half a view of margin: a real gain on the big song in both engines, none on the beat song; not kept in Z2.** The margin is the camera's number (`CULL_MARGIN`, `CULL_STEP`), shared with Notes, and Z1 leans on it: a live zoom stretches into the margin and lays the tracks out early only past it (about ×1.5 out now, ×1.33 with half a view). With Z1 in, a zoom frame no longer lays the tracks out, so the margin should be chosen then, from the end of a zoom and from a pan's renders, not from these frames. It is one line each in `numbers.ts`.

## 4. The decision

**Z1: while you pinch or ⌘-scroll the tracks, the picture stretches for a moment, then snaps sharp when your fingers stop. Yes or no?**
- **Yes:** a pinch on a beat song runs near 120 frames a second.
- **No:** we do only Z2. Then a pinch on a beat song costs about 25 ms a frame in WebKit (an estimate from the measurements): about 40 frames a second on your Mac, better than today's 20 to 25.

Z2 changes nothing you see, so it goes ahead either way.

**Decided, 2026-11-14: yes.** The owner left the choice to the lead, who chose yes:
- Smooth motion under the fingers was the owner's first ask for the camera, and Z2 alone stays near 40 frames a second.
- Apps on the Mac zoom this way: Maps, Preview, Safari's pinch. The stretch lasts only while the fingers move.
- It can be taken back. A view that marks nothing to stretch zooms as today, so if the stretch looks wrong on the owner's Mac, the views stop marking it.

## 5. The rules that change

| Rule | Today | After |
|---|---|---|
| VIEW-5 | A zoom lands in the frame it was asked in. | With Z1: a zoom by the hand shows in the frame it was asked in, stretched; the tracks are laid out at it when it ends. A zoom by the app (⌘=, Fit) is laid out at once, as today. |
| VIEW-8 | No React component renders on a frame the camera moves, except at a step of the margin. | Also: no React component renders on a frame of a live zoom. |
| VIEW-12 | Clips draw their notes as SVG; the 2 px marks by `data-fine`. | Clips draw their notes on canvases, one a lane (§3, built), redrawn when their notes change or the zoom settles. |

## 6. Build order

1. **`zoom-bench`.** The bench joins the repo as a nightly-tier measure, so later changes can be compared: `e2e/zoom-bench.mjs`, Chrome only, plus the WebKitGTK driver as a tool. It also adds a step to the swipe probe that replays a pinch and reports the per-frame split, so the owner's Mac can be measured one for one.
2. **`canvas-clips` (Z2).** Clips' canvas drawings, the Graph's nodes kept still on a zoom, the grid once per zoom. Then the margin is measured again.
3. **`live-zoom` (Z1),** if the owner says yes. It lives in the camera library: a view marks what stretches, and the camera keeps the zoom shown beside the zoom laid out.

Each track has a builder, a skeptic's review and the full check, as the camera's did.
