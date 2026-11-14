# The camera: views that move as the hand does

| | |
|---|---|
| **Status** | Proposed, 2026-11-11. Revised after the skeptic's review (C1 to C33, answered in §15). The owner said yes, and changed it on 2026-11-12 after trying PR #112 (§16): tracks 2 and 3 are one track, `camera`, with the camera as a library and the views drawing only what is in view. |
| **Written** | 2026-11-11, from `main` at `18e52c44`. The owner asked for it in these words: "the scrolling and panning doesn't feel very smooth on macOS, for both the tracks and devices; can you make sure they feel as native as possible", "it doesn't scroll diagonally", "I want React to still be like the source of truth, and so that I don't have to deal with like native apps, and also might benefit on Linux platforms", "if I would re-architect the scrolling UI from first principles, would you go with the camera option?", and "go ahead and do it then, using best practices". |
| **Replaces** | PR #112 (a diagonal swipe scrolled by `web/src/lib/freeScroll.ts`), which closes unmerged, and the browser's own scrolling in the four views. §11 maps every rule. |
| **Beside it** | [inventory.md](inventory.md): everything that depends on the four views' scroll boxes today, with `file:line`. |

**In short**
- **Four views get a camera:** the arrangement, the Graph, Notes and the beat grid. A camera is three numbers, where the view looks (across and down) and its zoom, kept in the app's state. It is the one truth: what each view shows follows from it.
- **Every swipe over them is the camera's,** in any direction, diagonal included. A swipe that is nearly straight keeps to its rail, so the tracks don't drift sideways as you scroll down.
- **The browser still thinks the view scrolls; the camera tells it where.** Each view keeps its scroll box, and the camera writes the box's scroll position. So the sticky ruler and names, Tab, the waveforms that draw when seen, and every browser suite keep working. When the browser moves a box itself (Tab, a key, a swipe it took over), the camera takes the new position in.
- **It feels like the Mac:**
  - the view moves 1:1 with the fingers, written once a frame;
  - the system's glide after a flick, on the Mac; the camera's own glide on Linux, where the page gets none;
  - pinch and ⌘-scroll zoom around the pointer in all four views;
  - 120 Hz on a 120 Hz Mac (D4).
- **Moving the view redraws nothing.** No React component renders on a frame the camera moves. A frame meter shows it, and a swipe probe measures what the Mac's web view really sends, before anything is built.
- **First a probe, then the camera in all four views.** Track 0 measured today's build on the owner's Mac. Track `camera` builds the camera as a library, `web/src/camera/`, and moves the four views onto it at once, with no switch: the owner compares against `main` (§16).
- **The views draw only what is in view.** The arrangement and Notes draw the clips and notes in view and about a view's width more each side; the window moves in steps, so a swipe redraws only when it crosses one (§8).
- **The core gains nothing.** Rust changes only for 120 Hz on the Mac (D4): one switch in the Mac shell.

## 1. What is wrong today

- **A swipe moves one way only.** WebKit, the web view the app runs in on the Mac, passes every swipe through AppKit's "predominant axis" filter, which keeps only the way the swipe mostly goes (`WheelEventDeltaFilterMac.mm`). WebKit has no setting to turn it off. Native views that move freely turn it off for themselves (`NSScrollView.usesPredominantAxisScrolling`); a web page can't. WebKit on Linux uses a simpler filter of its own (`BasicWheelEventDeltaFilter`). The page's own wheel events are not filtered: they carry both directions.
- **PR #112's patch can't be made smooth.** It moved the view itself only for the events of a swipe that went both ways, and left the rest to the browser, so one swipe flipped between two engines. Worse, WebKit lets a page cancel a swipe only at its first event (`WheelEventGesturesBecomeNonBlocking`): a swipe that started straight and turned diagonal was moved by both, twice as far one way.
- **The views fight their scroll boxes.**
  - The arrangement keeps a `spare` width and a `flushSync` so that a zoom has room to scroll to (`SongTimeline.tsx:473-482`). It keeps an `ours` and an `across` to tell its own scrolls from a hand's (`430-467`), and reads `scrollLeft` in eleven places.
  - The Graph measures room for scrollbars that come and go (`NodeGraph.tsx:166-181`, `266-273`), though Fit no longer uses it (`281-283`).
  - Notes and the beat grid copy `scrollLeft` into their velocity strips by hand (`NoteView.tsx:1239`, `BeatView.tsx:353-356`).
- **Zoom is uneven.**
  - The arrangement's pinch renders the whole timeline on every gesture event, through `flushSync`.
  - The Graph's pinch and ⌘-scroll don't zoom around the pointer (`NodeGraph.tsx:202-215`).
  - Notes and the beat grid have no pinch.
- **The window redraws five times a second.** The status poll sets a new object every 200 ms, playing or not (`GraphCanvas.tsx:57-69`). GraphCanvas renders, and with it the arrangement, which isn't memoized (`SongTimeline.tsx:364`). The playhead already moves by itself each frame (TL-9).
- **The Mac's 60 Hz.** WKWebView in an app (not Safari) draws the page at most 60 frames a second on macOS 13 to 15, also on a 120 Hz screen (`PreferPageRenderingUpdatesNear60FPSEnabled`, on by default; [WebKit bug 294338](https://bugs.webkit.org/show_bug.cgi?id=294338)). The browser's own scrolling isn't capped: it runs on the screen's own rate (`RemoteLayerTreeEventDispatcher.mm:465`). Reports differ on macOS 26.

## 2. How others do it

| Who | How the view moves | What we take |
|---|---|---|
| Safari, Kinopio | the browser's own scrolling | It is the smoothest, but every swipe keeps to one direction. Not enough for a DAW. |
| VS Code | its own scroll position, and every wheel event is its own; overlay scrollbars that fade after 500 ms; rails for nearly straight swipes (`scrollPredominantAxis`, on by default, "prevents horizontal drift"). Its editor slides a layer and draws only the lines in view. Its panels and lists (`DomScrollableElement`) write their position into the element's `scrollLeft` and `scrollTop` instead. | The rails, the fade, and `DomScrollableElement`'s way of drawing. |
| tldraw, Excalidraw, Figma | a camera in the app's state; every wheel and pinch is theirs; Safari's gesture events for a pinch on the Mac, Ctrl + wheel elsewhere; draw only what is on screen; no hover checks while the camera moves | The camera, the pinch, no hover while moving, and "moving costs nothing". Their layers moved by transforms suit an endless canvas with no sticky rows, which our views are not (§4). |
| `wheel-gestures` (Embla), `@use-gesture` | a recognizer that guesses a swipe's start, end and glide from the events' timing | The idea, in a few lines of our own: no new dependency. |
| Apple's scroll views | 1:1 with the fingers, the system's glide, a stretch at the edges and a spring back | The feel to match. |

## 3. The camera

- **What it is.** Each of the four views has one camera: `{ x, y, zoom }`.
  - `x` and `y` are the top-left of what shows, in the content's pixels at the current zoom, as `scrollLeft` and `scrollTop` are. They keep fractions of a pixel.
  - `zoom` is the view's own unit: pixels a beat in the arrangement, Notes and the beat grid; the scale in the Graph.
  - The arrangement's zoom stays in `view.json` (FMT-20), as today. Where the view looks isn't saved, as today.
- **Where it lives: a library of its own, `web/src/camera/`** (§16).
  - `index.ts` is the only file the app imports. The library imports nothing from the app (not `lib/`, not `components/`, not the app's stores); it uses React and zustand only. A check in the web build (`web/scripts/camera-bounds.mjs`, run by `npm run typecheck`) refuses any import in it that reaches outside it.
  - Its `README.md` says what it does and how a view uses it.
  - It holds the cameras (one small zustand store each, by name: one per open view); the pure functions below; the recognizer (§5); `useCamera`, the writer (§4), and the `moving` flag; and the culling helper (§8).
  - The app tells it the platform (a Mac or not) with `setPlatform`. It never reads `navigator` itself.
  - Per-frame values (`x`, `y`) are read by subscription callbacks, never by a hook that renders.
  - Hooks select only coarse values: the zoom, the culling window's step, whether the camera is moving.
  - Its numbers (G, the rails, the glide, the culling's margin and step) are named constants in one file, `numbers.ts`, so the probe's report changes one line each.
- **What it knows.**
  - The view's size, from a ResizeObserver (a browser callback when a box changes size), never read while moving.
  - The content's size: the song's room in the arrangement (`room()`), 128 rows in Notes, the layout in the Graph.
  - Its margins: the fixed parts that cover it (the ruler, the track names, the Song row, the keys), so a move that reveals something puts it clear of them.
- **What it does,** as pure functions, each with checks of its own:
  - `move(dx, dy)` and `to(x, y)`, clamped to the content;
  - `zoomAround(point, zoom)`: what is under the point stays there;
  - `reveal(box)`: the least move that shows a box clear of the margins;
  - `fit(range)`: a range fills the view (Z, Fit);
  - `toContent(clientX, clientY)` and back: the conversion for new code;
  - `cull`: what part of the content is in view, plus a margin, in the content's units, moving in steps (§8).
- **Who moves it,** and every move says which:
  - the hand: a swipe, a pinch, the scrollbars;
  - the app (§7): Follow, Fit, Z, Show, a marker, a new lane, an expanded node, a lit column;
  - the browser: a move it made to the box itself (§4).

  Follow pauses on any move that isn't the app's own (Q11).

## 4. Drawing: the browser still thinks the view scrolls

- **The box stays a scroll box.** The camera sets its scroll position; nothing slides a picture of it. The browser doesn't scroll it on its own, because the camera cancels every wheel event over it (§5). Its own scrollbars are hidden only if the probe says so (§6).
- **The writer** (`useCamera`, one per view):
  - It writes only when the camera changed since its last write. It writes the content's size first, if that changed, then `Math.round` of `x` and `y`, and remembers what it wrote.
  - A move with no zoom is written from `requestAnimationFrame` (the browser's once-a-frame callback).
  - A zoom changes both the drawing and the position, so it lands in one frame. The store's update renders the view, and the writer writes the position in that render's layout effect (React's hook that runs before the screen updates). That replaces `flushSync` and `spare`.
- **What the browser moves is taken back in.**
  - It may still move the box itself: Tab bringing a focused bar into view (DV-15), a rename field's autofocus, the arrow and page keys on the focused timeline, a test's click, and a swipe WebKit took over (C1).
  - On the box's `scroll` event, the camera reads `scrollLeft` and `scrollTop`. If either differs from what it last wrote by more than 1 px, that is the browser's move, and the camera takes it.
  - The 1 px is needed because WebKit keeps scroll positions in whole pixels (`Element+CSSOMView.idl`: `long scrollTop`), while the camera keeps fractions. The camera never compares the box with its own position, only with what it last wrote. So a slow swipe never loses its fractions.
  - The writer takes the browser's move in before it writes, so it never writes over a key's scroll animation.
- **Why not slide layers?** [The inventory](inventory.md) found what needs a real scroll box:
  - the sticky ruler, names, keys, Song row and tools, in four views;
  - IntersectionObservers (the browser's "is this on screen?" callbacks), which draw waveforms (`Wave.tsx:167`) and ask for scopes only while seen (TAP-16);
  - every `scrollIntoView`;
  - Tab's focus;
  - Puppeteer's clicks, which scroll the box before they press, in about forty browser suites.

  With layers, each of those is rebuilt, and a box with `overflow: hidden` still moves under the layer when focus or a click scrolls it. And when WebKit takes a swipe over (C1), only a real scroll box gives a working fallback.
- **What a write costs:** no repaint of what is drawn, since the box is composited (the GPU moves it). A write still settles pending layout, so the frame meter's slowest frame is the judge (§8).
- **Small guards:**
  - `overflow-anchor: none`, so the browser never shifts the box when rows above change height;
  - `overscroll-behavior: none`, so nothing reaches the window.

## 5. Input: what the page can see, and the recognizer

**What the page can see.** A wheel event in the page has movement (deltas), a time and the modifier keys, and nothing else. It has no phase, so the page can't tell fingers down, fingers up or the end of a glide; WebKit doesn't send the page events with no movement at all (`Element.cpp:642-651`). So a **swipe** here is a run of wheel events less than **G** ms apart. G is measured on the owner's Mac by the probe (track 0): 50 ms (2026-11-14, §16 item 10).

**One listener per view,** in the capture phase (it hears an event before anything inside the view does), not passive.
- **Whose swipe it is,** decided at its first event:
  - ⌘ or Ctrl is down: a zoom;
  - the event's target is inside an element marked `data-wheel`: that control's swipe, to its end. The marked elements are a picture's dot (DV-17), a Mix knob, and the effect browser's list;
  - otherwise the camera's, to its end. Each event is cancelled and stopped there (`preventDefault`, `stopPropagation`), so no control the content carries under a still pointer ever sees it (C3).
- **An event that can't be cancelled is the browser's.** WebKit waits at most 50 ms for the page to answer a swipe's first event; if the page is busier than that, WebKit scrolls the swipe itself and marks every later event `cancelable: false` (`RemoteScrollingTreeMac.mm:335-358`). The camera doesn't move by such an event, and takes the box's move in (§4). The frame meter counts these swipes.
- **1:1, both ways at once.** The camera moves exactly as far as each event says. The system's acceleration is already in it, and WebKit gives pixels. Chrome's lines and pages, which only the suites see, become pixels as today (×33 a line).
- **Rails.** If a swipe's first 16 px go within 25° of an axis (their sum with signs, so a hand's jitter cancels), it starts on that axis's rail; it breaks free for the rest of the swipe once its last 100 px have turned more than 25° off the rail, all told, and moves both ways from there (what was held back isn't moved), as Chromium's touch rails do. A long swipe's slight slant, or its jitter, never adds up to a break (review Q, Q3: a lifetime budget of 24 px off broke a 3° swipe after 458 px). A swipe further from an axis moves both ways. The Graph has no rails: a canvas pans freely (Figma, Freeform, tldraw). The numbers are set from the owner's probe (§16 item 10).
- **⇧ + wheel** is never turned by the camera: the Mac and WebKitGTK already turn it sideways.
- **Once a frame.** The events of one frame are summed and written once. Nothing is read from the page on the way.
- **The glide.**
  - On a Mac, the glide after a flick reaches the page as more wheel events, both ways. On recent macOS they are AppKit's own; on older paths WebKit builds them from the unfiltered deltas (`RemoteLayerTreeEventDispatcher.mm:280-297`, `EventDispatcher.cpp:140-208`). The camera moves by them like any other. The probe confirmed it (2026-11-14): they still come, and stay cancelable, when the swipe's first event was cancelled.
  - On Linux, WebKitGTK glides inside itself and gives the page nothing to glide by (`ScrollingEffectsController.cpp:337-388`). So there the camera glides itself, decided by the platform, not guessed. A swipe's glide starts one frame after its last event, at the speed of its last 50 ms, and slows as Apple's iPhone scroll views do (UIScrollView's normal rate, 0.998 a millisecond). An event that comes late takes over from the glide.
  - Any new wheel event, a click or a key stops the camera's glide.
  - A move the browser made is heard on the box's `scroll`, a frame later. What decides from where the view is (Follow) takes it in first (`sync`), so a move made before Follow listened is not heard as one made while it follows.
  - After a click, a key or a move by the app, wheel events are dropped until a gap of G ms. That is how a glide still arriving from the system stops.
- **Drags hold the camera still.** While a pointer drag holds the pointer (a clip, a marquee, a note, a cable, a fade, a reorder, the ruler's loop), the camera doesn't move by a swipe, a glide or Follow. Follow catches up after the release. The eight drags that measure from where they started (inventory §7) stay as they are.
- **No hover while moving.** While the camera moves, and for 250 ms after, the views' hover handlers do nothing (a `moving` flag). WebKit sends a fake mouse move after a scroll, and Notes' and the wires' hover would otherwise render.
- **Zoom.** ⌘ + wheel zooms around the pointer in all four views: the arrangement, Notes and the beat grid in time, the Graph in scale. A pinch does the same on a Mac (Safari's `gesturestart`, `gesturechange` and `gestureend` events) and in Chrome (Ctrl + wheel).
  - **On Linux,** WebKitGTK gives a pinch to its own page zoom, not to the page. Pinch there is "not yet": the probe on a Linux laptop says what happens, and the shell turns that page zoom off if it magnifies the window (gaps.md says so until then).
  - The keys stay as they are: ⌘= and ⌘− around the playhead or the middle; Z and Z again (TL-11, Q9). Notes' and the beat grid's zoom buttons zoom around the middle.
  - A pinch's last scale is kept when the fingers lift.
- **Edges: a stop** (D3). The view stops at each edge on every system. Apple's stretch and spring back are a later track, if the owner misses them after trying the camera. To be right, the stretch must move only the lanes, keep the ruler and names still, and portal the effect browser first (C12).

## 6. Scrollbars

- **The browser's own bars by default.** Under the camera, dragging one is a browser move, taken in (§4). So the bars keep working, with their look, their accessibility, and nothing to build.
- **The probe of 2026-11-14** saw the bars in every step, for the page's own moves too. But that Mac shows its scroll bars always, so the overlay bars (the Mac's default with a trackpad) are not measured yet (gaps.md).
- **Our own only if the probe shows a need** (D5): if the Mac's overlay bars don't appear for the camera's moves, so you can't see where you are. Then the views get thin overlay bars:
  - 6 px, growing to 10 px under the pointer, shown while the camera moves or the pointer is over them, gone 500 ms after (VS Code's time; the fade uses taste.md's `exit` token, and none with reduced motion);
  - the vertical bar from under the ruler to above the Song row, the horizontal one from the names' edge, clear of Follow's and the zoom's buttons;
  - `role="scrollbar"`, `aria-controls` and `aria-valuenow`; a hidden bar takes no clicks.

## 7. The app's own moves

Every move the app makes goes through the camera. None calls `scrollIntoView` or writes `scrollLeft`.

| Move | Today | With the camera | Track |
|---|---|---|---|
| Follow (Q11) | pages at 85% to 15%; a hand's scroll pauses it, guessed by `ours` and `across` | the same pages; any move not the app's own pauses it | 2 |
| ⌘=, ⌘−, pinch, ⌘-scroll (TL-11, Q9) | `zoomScroll`, `flushSync`, `spare` | `zoomAround` | 2 (arrangement), 3 |
| Z, Fit (TL-11, DV-5) | `zoomFit`, `fitted` | `fit`, and Z back to the camera before | 2, 3 |
| Show for bars, a lane, a clip, a track (AGENT-6) | `showBeats`; `scrollIntoView` | `reveal`; bars from 15% in when they fit, as today | 2 |
| Show for a knob (AGENT-6, DV-16) | `expandFor`, then `scrollIntoView` (`pointer.ts:143-153`) | `expandFor`, then the Graph's `reveal` | 3 |
| Add Marker | scrolls the bar to 15% | `reveal` with the same margin | 2 |
| A new lane or track (TL-16) | `scrollIntoView` with `scroll-padding` | `reveal` with the margins | 2 |
| Track heights, ⌘↑ and ⌘↓ (TL-11) | the content grows or shrinks under the view | as today: the top row stays where it is | 2 |
| An expanded node (DV-13), a lit column (DV-18) | `scrollIntoView` after the move | `reveal` after the move | 3 |
| Notes opens | centres the clip's pitches | `to(…)`, as today | 3 |
| Notes' row height, ⌘↑ and ⌘↓ (NOTE-9, NT-5) | writes `scrollTop` to keep the middle row | `to(…)`, keeping the middle row | 3 |
| Notes' and the beat grid's zoom buttons | anchor and layout effect | `zoomAround` the middle | 3 |

- **What agents see** (AGENT-6's "the bars the timeline shows") is read from the camera, rounded as today (`focus.ts`), and not from a scroll event.
- **The pointer's note** (`PointerNote.tsx`) moves on each frame the camera moved, and not otherwise: it hears `onViewMoved` (right after each write) and writes its place into its own style, rendering only when what it shows changes.
- **The scroll padding stays** (TL-16). The app reveals through the camera; the padding keeps the browser's own reveals (Tab, autofocus, a test's click, a drag) clear of the headers.

## 8. Speed

- **No React render on a frame the camera moves.**
  - A move writes two numbers to the DOM.
  - Notes' and the beat grid's velocity strips, and agents' `seen`, read the camera by subscription.
  - A render is allowed only when the view crosses a bar (the beat grid's steps near what shows, KIT-10) or shows a waveform tile for the first time (`Wave.tsx:161-170`).
- **The status poll stops rendering the window.** The poll's status goes into a store, read by selectors (`playing`, the loop), as the app's other stores are. The arrangement takes no `beat` prop: Follow and the room after the playhead read the clock (TL-9's `onFrame`), and the room grows a bar at a time, as today.
- **A zoom renders at most once a frame,** and lands in that frame (§4).
- **The budget is one frame at the screen's rate:** 16.7 ms at 60 Hz, 8.3 ms at 120.
  - It is measured with the frame meter during a pinch, on the owner's Mac, on a song the probe generates: 24 tracks of 64 clips.
  - The owner saw it missed on #112's branch (§16), so the views draw only what is in view, in this track. A CSS variable for the zoom saves React's render but not the layout of every clip, so it alone isn't enough.
- **Drawing only what is in view** (VIEW-12).
  - The library's `cull` gives a view the part of its content in view plus a **margin**, in the content's units (beats across; pixels or rows down). The margin is one view's width (or height) each side.
  - The window moves in **steps**: it is kept while it holds what shows and half a view more each side, and isn't twice as wide as a new one. So a pan makes a new one every half a view, and a zoom every ×1.5 out or ×2 in, never every frame: a swipe renders only then, as VIEW-8 allows a bar crossed.
  - **A new window never holds up a frame.** It is set in a React transition, which React draws between frames and drops for newer work; the margin lets it lag. Only if what shows has already left the window is it drawn at once. It draws only the clips entering it: each clip's box is memoised by its own data and kept.
  - **A zoom renders no clip.** Each clip's box (and a control lane's fallback line) is placed by the zoom straight into its own style (`useSpan`), from one subscription to the camera. A CSS variable for the zoom on the lanes' box did the same with no render, but a change to a variable reaches every element under it: Chrome worked out the whole arrangement's style anew each zoom frame (1.7 s over 30 frames on the big song, against 0.16 s), so it was dropped.
  - **The arrangement** draws only the clips that overlap the window across. Tracks more than a view's height above or below draw no clips, but keep their row boxes, so heights, the sticky names and Tab's order stay. The ruler's bar numbers are drawn only in the window too; the grid's lines are one pattern a time signature, so they cost nothing to keep.
  - **Notes** draws only the notes in the window, across and down, and their velocity bars across.
  - **The beat grid** kept its own culling by bar (KIT-10): it now reads the camera's window, with its own margin of a bar's room (4 beats) each side, made anew when that room falls under 2 beats, so it still draws no step far out of view.
  - **The Graph** is small: no culling.
- **A clip's mini drawing doesn't render on a zoom.** The notes, the automation curve and the steps inside a clip are SVGs whose `viewBox` is in beats, or boxes placed in percent of the clip. They don't read the zoom, and are memoised by their clip, so a zoom changes only the clip's box. A note under 2 px wide is drawn 2 px wide by a line that doesn't scale with the drawing (`vector-effect: non-scaling-stroke`). An audio clip's waveform is drawn in pixels, so it does redraw on a zoom.
- **The frame meter.** "Show frame rate" in the command palette shows, over the window:
  - frames a second;
  - the slowest frame of the last second;
  - the React renders of the last swipe, counted by one `useRenderCount` line at the top of each view and its rows (React's own Profiler does nothing in the app's production build);
  - the swipes WebKit took over (§5).

  Suites read the same numbers from `canvas.frames()`.
- **120 Hz on the Mac** (D4), done by track `mac-feel` (VIEW-11).
  - The Mac shell turns off WebKit's `PreferPageRenderingUpdatesNear60FPSEnabled` when it makes the window, through `WKPreferences`' private `_features` API, checked with `respondsToSelector` first, as `tauri-plugin-macos-fps` does. If the API is gone, the shell logs one line and keeps 60 Hz.
  - It matters more under the camera, since every swipe now draws on the page's frames: without it, a straight swipe on a 120 Hz Mac with macOS 13 to 15 would draw at half today's rate.
  - It doubles the work of every once-a-frame loop (meters, the playhead), which the frame meter shows.
  - It is fine for a downloaded app. The Mac App Store would refuse it.
  - It lives in `apps/music2-desktop` (PLAT-2's shell exception). `objc2` is already in `Cargo.lock` through Tauri, but a direct dependency is still a dependency change, in its own pull request first (AGENTS.md rule 7).

## 9. Tests

- **The camera's functions** have their own checks: `move`, `zoomAround`, `reveal`, `fit`, the rails, the glide, and the writer against a fake box that keeps whole pixels, as WebKit does. They run in a browser suite, since `web/` has no unit runner.
- **A browser suite, `camera`, written first.** The camera now handles a suite's synthetic wheel events in Chrome too. Chrome can't show WebKit's own behaviour, so where it can't, the suite sends the events WebKit would send. It checks, in each of the four views where they apply:
  1. a diagonal swipe moves both ways, 1:1; a swipe 10° off straight keeps to its rail; a 45° one doesn't; since 2026-11-14, the owner's own swipes from the probe (`e2e/fixtures/swipes`): the straight ones on their rails, the diagonal ones free;
  2. events with `cancelable: false` don't move the camera, and the box's own move is taken in;
  3. a swipe that carries a picture's dot under the pointer changes no file;
  4. pinch, as Safari's gesture events with `scale` and `clientX`, and ⌘-scroll keep the beat, or the node, under the pointer;
  5. 120 events of a swipe cause no React render, by `useRenderCount`, with the pointer over notes and over wires;
  6. a swipe, a key's scroll, and a write to `scrollLeft` while playing each pause Follow (`pl-zoom`'s check, kept);
  7. during a clip drag, a swipe and Follow move nothing;
  8. agents' `seen` follows the camera;
  9. Tab to an off-screen bar, and a test's click, are taken into the camera;
  10. the camera's glide, with Linux's flag set in the page: it starts after a swipe, slows, and stops at the next event;
  11. the library's pure functions against known numbers, run in the page;
  12. culling, on the 24-by-64 song: the arrangement holds no more clips than the view and its margins can show; a swipe of 120 small events renders the rows only when it crosses a step; 30 ⌘-scroll events render the arrangement at most once a frame; the same for Notes with a clip of many notes; a zoom doesn't render a clip's mini drawing.
- **The suites that read or write `scrollLeft`** keep working, since the box still scrolls and its moves are taken in (§6 of [the inventory](inventory.md)).
- **The probe's report** (track 0, 2026-11-14) is kept as what a later change compares with: its wheel events, with nothing about the Mac, are the `camera` suite's `e2e/fixtures/swipes/owner-2026-11-14.json`, and §16 item 10 says what it showed.
- **By hand, on the owner's Mac,** after the track, against `main`:
  - swipe straight, diagonal and turning, and watch for drift;
  - flick, flick and hold, and stop a glide;
  - swipe while the song recompiles;
  - glide over a picture in the Graph;
  - hit each edge;
  - pinch over a clip and over a node;
  - read the frame meter while a song plays.

## 10. What goes

All of it in track `camera`, from all four views at once (§16):
- PR #112: `freeScroll.ts` and its `scroll` suite (its checks move into `camera`).
- In the arrangement:
  - `spare`, `zoomScroll`'s `flushSync`, `ours` and `across`;
  - the `scroll` listener for `seen`;
  - the `beat` prop;
  - its own wheel and gesture listeners for the zoom;
  - the wrong comment that WebKitGTK sends a pinch as Ctrl + wheel (`SongTimeline.tsx:591-592`).
- In the Graph: `scrollbarRoom()` and `room0`, which Fit no longer reads; track `mac-feel`'s `anchor` and `zoomAround`, and its own wheel and gesture listeners.
- In Notes and the beat grid: the `onScroll` mirrors, the beat grid's `left` state, and each one's zoom `anchor`, its layout effects and its own wheel listener (Notes' row anchor too).
- Every `scrollIntoView` in the app's own code: `SongTimeline.tsx:1746`, `pointer.ts:138` (for a lane, a clip, a track and a knob), `NodeGraph.tsx:1241` and `PictureView.tsx:332`.
- There is no switch: the owner compares against `main`.

## 11. The rules: kept, rewritten or added

The contract changes first, in track `camera`'s first commit. The new rules are VIEW-1 to VIEW-12, in a new taste.md §3.10, "Moving the view" (VIEW-10 came with track 0, VIEW-5 and VIEW-11 with track `mac-feel`, VIEW-12 with §16).

| Rule | Becomes |
|---|---|
| **VIEW-1** (new) | One camera per view (the arrangement, the Graph, Notes, the beat grid): where it looks and its zoom, the one truth of what it shows (§3). |
| **VIEW-2** (new) | A swipe is a run of wheel events less than G ms apart. A swipe over these views is the camera's, unless it starts on a `data-wheel` control or with ⌘ or Ctrl. The browser scrolls none of the camera's swipes, except one WebKit takes over (`cancelable: false`), whose move is taken in (§5). |
| **VIEW-3** (new) | 1:1 with the fingers, both ways at once, a nearly straight swipe on its rail, written once a frame. |
| **VIEW-4** (new) | The glide: the system's on the Mac; the camera's own where the page gets none (WebKitGTK), stopped by the next event, a click or a key. |
| **VIEW-5** (new) | ⌘-scroll zooms around the pointer in all four views, and so does a pinch on a Mac; on Linux, pinch isn't supported yet. |
| **VIEW-6** (new) | The view stops at its edges. |
| **VIEW-7** (new) | The scrollbars: the browser's, or our own if the probe shows a need (§6). |
| **VIEW-8** (new) | No React render on a frame the camera moves, except a bar crossed or a tile first seen. The status poll doesn't render the window. A zoom renders at most once a frame, and lands in it. While a pointer drag holds the pointer, the camera holds still. |
| **VIEW-9** (new) | The app moves these views only through the camera (§7); a move by the browser itself is taken in. Follow pauses on any move that isn't the app's own. |
| **VIEW-10** (new) | The frame meter and the probe. While the probe runs, the camera leaves plain swipes to the web view (§16). |
| **VIEW-11** (new) | The page at the screen's rate on a Mac (D4). |
| **VIEW-12** (new) | The views draw only what is in view, plus a margin of a view each side, the window moving in steps of half a view (§8). A clip's mini drawing doesn't render on a zoom. |
| taste.md §2.13 (no motion for the playhead, drags, zoom and scroll) | **Rewritten:** no motion except the camera's glide (VIEW-4) and our scrollbars' fade, if built; with reduced motion, neither. |
| taste.md §3.8 ("Nothing scrolls horizontally except the arrangement, the roll, the Graph and the Mix strips") | **Rewritten:** adds the beat grid. |
| taste.md §5.2 (gestures) | **Rewritten:** rows for a swipe in any direction, a pinch and ⌘-scroll. |
| TL-9, TL-11, Q8, Q9, Q11, Q31, X25, X33 | **Kept.** Q11's "a scroll by hand pauses it" now means any move not the app's own (VIEW-9). |
| TL-16 (scroll padding, `block: "nearest"`) | **Rewritten:** the app reveals through the camera with margins; the padding stays for the browser's own reveals. |
| DV-5 ("a graph bigger than the dock scrolls"), DV-14 (the wheel scrolls the graph) | **Kept**, through the camera. |
| DV-15 (Tab goes through the nodes) | **Kept:** the browser scrolls to the focused one, and the camera takes the move in. Its *Today* line says so. |
| DV-17, CARD-70 (a picture's dot keeps its wheel) | **Kept**, by the dot's `data-wheel` marker (§5). |
| DV-13, DV-16, DV-18, AGENT-6 (scroll a node, a value, a column into view; Show) | **Kept**, through `reveal`; their *Today* lines name it. |
| DV-10, TAP-4, TAP-8, TAP-16 (ask only while seen) | **Kept:** the box still clips, so the IntersectionObservers still see. |
| KIT-10 (the beat grid scrolls and zooms by ⌘ + wheel and its buttons) | **Rewritten:** and by a pinch; its culling reads the camera. |
| NOTE-8, NOTE-9, NT-5 (rows and their height) | **Kept.** NOTE-9's and NT-5's *Today* lines say the row height keeps the middle row through the camera. |
| FMT-20 (`view.json`: zoom, follow) | **Kept.** |
| PLAT-2 | **Rewritten:** the web UI also differs in its glide, the camera's own off the Mac (from `isMac`). The 120 Hz switch is in the shell, which PLAT-2 already allows. |

## 12. What the core gains

Nothing in the engine, the editor, the server or the file formats. In `web/`:
- `src/camera/`, a library of its own: the cameras, their functions, the recognizer, `useCamera` (the writer), the `moving` flag and the culling helper;
- the status store (`lib/status.ts`), so the poll redraws only what shows it;
- `useRenderCount`, the frame meter and the probe (track 0);
- our own scrollbars, only if D5 needs them.

In Rust, only D4's switch.

## 13. Build order

Each track has a builder, a skeptic and the builder again, and each changes the contract first.

| # | Track | Builds | Size |
|---|---|---|---|
| 0 | `camera-probe` | The frame meter and the probe, on today's views. The probe, from the command palette, logs each wheel event's deltas, whether it can be cancelled and its gap from the last, the gesture events, the frame rate, and whether the Mac's scrollbars show, first with the swipe left to the browser and then with it cancelled. It also generates the 24-by-64 test song. The owner runs it on the Mac and pastes its report. That report sets G, the glide, D4 and D5. | small |
| 2 | `camera` | The contract (§11, §16). The library `web/src/camera/`: the cameras, `useCamera`, the recognizer, the culling helper; the status store. All four views on their cameras, with no switch: zoom around the pointer and pinch in each, Fit, Z, Follow, Show (for bars, a lane, a clip, a track and a knob), markers, `seen`, TL-16's reveal, DV-13's and DV-18's reveals, Notes' rows, the strips, drags holding it still, `data-wheel` on a picture's dots. Drawing only what is in view (§8). The `camera` suite. PR #112 closed. | large |

120 Hz (D4) was done by track `mac-feel` (VIEW-11), so it is no longer a track here. Tracks 2 and 3 became one (§16). After it, the owner tries it on the Mac with the frame meter, against `main`.

**Not in this note:**
- The stretch at the edges (C12 says what it needs).
- Pinch on Linux, until the probe has run there.
- The Mix strips, which scroll one way and keep the browser's scrolling.
- The code view, which is Monaco's.
- A Follow that scrolls smoothly instead of paging.

## 14. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | The four views get a camera, and every swipe over them is the camera's. All right? | **Yes.** It is your own choice (2026-11-11). |
| D2 | How the camera shows the view. **(a)** The browser still thinks the view scrolls, and the camera tells it where, as VS Code's panels do. **(b)** The camera slides a picture of the view itself, as tldraw does. | **(a).** The fixed ruler and names, Tab, the waveforms and forty test suites keep working, and when WebKit takes a swipe over, (a) still scrolls right. (b) would rebuild all of those. |
| D3 | At an edge, the view stops, on every system, for now. Apple's stretch and spring back come later, as their own small track, if you miss them. All right? | **Yes.** Done right, the stretch must move the lanes but not the ruler or the names; it is better built after you've felt the rest. |
| D4 | 120 Hz on the Mac, through WebKit's hidden setting. It is safe outside the Mac App Store; if Apple removes the setting, the app keeps 60 Hz. **Which Mac, and which macOS, do you use?** | **Yes,** if your Mac has a 120 Hz screen (a MacBook Pro from 2021 on). Then it comes before the camera, or straight swipes could feel choppier than today. |
| D5 | Scrollbars: the browser's own, unless the probe shows the Mac doesn't show them for the camera's moves; then thin ones of our own. All right? | **Yes.** |
| D6 | Nearly straight swipes keep to their rail (within about 20°), so scrolling down the tracks doesn't drift sideways; others move both ways. All right? (Answered 2026-11-12, §16 item 9: within 10°, broken by a turn, none in the Graph. Since the probe, item 10: within 25° after 16 px.) | **Yes.** VS Code does the same, by default. |
| D7 | PR #112 closes unmerged. All right? | **Yes.** It is replaced whole. |

## 15. The review, answered

| Id | Finding | Answer |
|---|---|---|
| C1 | WebKit can still take a swipe over after 50 ms, and the camera would then move it twice | Agreed. An event that can't be cancelled is the browser's; the camera takes the box's move in (§5, VIEW-2). The meter counts such swipes; `camera` check 2. |
| C2 | The page never sees a swipe's start, end or a resting touch | Agreed. §5 opens with what the page can see. A swipe is a run of events less than G ms apart, G from the probe. The glide stops at the next event, a click or a key; events after a click, a key or an app move are dropped until a gap of G. |
| C3 | A swipe can turn a picture's dot | Agreed. A capture-phase listener decides at a swipe's first event from its target; `data-wheel` marks the controls; the camera's swipe is stopped there to its end. `camera` check 3. |
| C4 | Without D4, the camera may scroll at half today's rate | Agreed. D4 asks which Mac first, and comes before the camera when it matters (track 1). The switch lets the owner compare with today in one build. |
| C5 | The self-made glide fires on the Mac when it shouldn't, and starts late on Linux | Agreed. The camera's glide is for WebKitGTK only, decided by the platform; it starts one frame after the last event, and a late event takes over. No one here has a Linux laptop, so gaps.md says the Linux glide is checked only by suites until someone does. The rate is named as the iPhone's. |
| C6 | Whole-pixel `scrollLeft` | Agreed. The camera keeps fractions; the writer rounds and remembers; the scroll handler compares with what was written, ±1 px. A unit check with a truncating box. |
| C7 | Write only on change; a zoom must land in one frame | Agreed (§4). Takes browser moves in before writing; a zoom writes in the render's layout effect. |
| C8 | Follow must pause for the browser's moves too | Agreed (§3, §7, VIEW-9); `pl-zoom`'s check kept as `camera` check 6. |
| C9 | Removing `beat` doesn't stop five renders a second | Agreed. The poll's status moves into a store with selectors (§8). VIEW-8 allows a bar crossed and a tile first seen. |
| C10 | Hover handlers render while the camera moves | Agreed. A `moving` flag mutes them for the move and 250 ms after; check 5 swipes over notes and wires. |
| C11 | No way to count renders in a production build | Agreed. `useRenderCount` in each view and its rows. |
| C12 | The stretch drags the sticky headers and the effect browser | Agreed. The stretch is cut from this note (D3); what it needs is written in §5 for a later track. |
| C13 | On Linux a pinch zooms the whole page | Agreed. Pinch on Linux is "not yet", the probe measures it, the shell may turn the page zoom off, and the wrong comment goes. |
| C14 | ⇧ + wheel would be turned twice on Linux | Agreed. The camera never turns it. |
| C15 | Drags assume a still view | Agreed. While a drag holds the pointer, the camera holds still; Follow catches up after. `toContent` is for new code; the drags stay. Check 7. |
| C16 | No rails: straight swipes drift | Agreed. Rails within about 20° (D6), tuned on the Mac; check 1. |
| C17 | PLAT-2 lists the UI's OS differences in full | Agreed. PLAT-2 is rewritten for the glide. The stretch, the other difference, is cut. |
| C18 | §11 misses or mislabels rules | Agreed. DV-15, DV-17 and CARD-70, KIT-10 (rewritten), §2.13's glide and fade, §3.8's beat grid, NOTE-9's and NT-5's *Today* lines; the rules sit in §3.10, not under the keyboard. |
| C19 | The tests can't see what broke PR #112 | Agreed. The suite sends what WebKit would: events that can't be cancelled, a dot carried under the pointer, gesture events, a truncating box; the Mac checklist adds a busy swipe, a flick and hold, a turn, and a glide over a picture. |
| C20 | No probe and no "before" | Agreed. Track 0 is the probe and the meter, run on today's build; the camera comes behind a switch. |
| C21 | VS Code's editor isn't the precedent | Agreed. It is `DomScrollableElement`, VS Code's panels and lists (§2, D2). |
| C22 | A write isn't free like a transform | Agreed: "no repaint", and the meter judges (§4). |
| C23 | The 8 ms threshold measures the wrong thing | Agreed. The budget is one frame at the screen's rate, the slowest frame during a pinch, on a generated 24-by-64 song (§8). |
| C24 | D4 holds; `objc2` is already in the lock; the switch doubles every loop | Noted in §8. The dependency still gets its own pull request. |
| C25 | Our bars would sit over buttons and need roles | Agreed, if built (§6). |
| C26 | Keep TL-16's padding | Agreed (§7, TL-16 rewritten). |
| C27 | The classifier decides nothing | Agreed. Cut: the glide is decided by the platform. |
| C28 | The Mac's glide path described wrongly | Corrected in §5. |
| C29 | "Not every frame" would make the pointer note lag | Corrected: on each frame the camera moved (§7). |
| C30 | §7 misses moves | Added: Notes' rows, track heights, the zoom buttons, Show for a knob in track 3. |
| C31 | Lists inside a view: a marker, not a measurement | Agreed: `data-wheel` on the effect browser's list; no styles read. |
| C32 | Words for a beginner | D2 gives each choice in plain words; terms have a short gloss where they first appear. |
| C33 | Why our own scrollbars? | The browser's stay, unless the probe shows the Mac hides them for the camera's moves (D5). |

## 16. After the owner tried #112 (2026-11-12)

On #112's branch the owner found diagonal swipes work but don't feel native ("the pan still feels kinda unnatural on Mac"), and zooming the tracks or Notes drops the frame rate a lot on the Mac at 120 Hz. #112 moved only the events that carried both deltas and left the rest to WebKit, so one swipe had two owners; and every zoom step redrew every clip in the song, synchronously, once for each wheel event. Asked whether a pan-and-zoom library could do this: no. Those libraries replace the Mac's own scrolling, several miss Safari's pinch events, and none fixes that cost. The owner then asked to "package the UI into its own library so my code space could be cleaned up", and "alright, clean everything up". So:

| # | Change | Where |
|---|---|---|
| 1 | **The camera is a library, `web/src/camera/`.** `index.ts` is the only file the app imports; the library imports nothing from the app, only React and zustand, and a check in the web build refuses an import that reaches outside it. It has a `README.md`. It holds the cameras, the pure functions (`move`, `to`, `zoomAround`, `reveal`, `fit`, `toContent`, `cull`), the recognizer (swipes, rails, the `cancelable` rule, zoom by ⌘/Ctrl-scroll and pinch, its own glide only where the platform gives none), `useCamera`, the writer, the `moving` flag, and the culling helper. The app tells it the platform; it never reads `navigator`. | §3, §12 |
| 2 | **One track, no switch.** All four views move to the camera at once (the arrangement, the Graph, Notes, the beat grid), and their own scroll and zoom code goes: §10's whole list, the Graph's `anchor` and `zoomAround` from track `mac-feel`, and Notes' and the beat grid's anchors and layout effects. The switch and the hand test "with the switch on and off" go: the owner compares against `main`. | §10, §13 |
| 3 | **Drawing only what is in view is in this track** (it was "not in this note, unless the meter shows a missed frame": the owner has seen the missed frames). The arrangement draws only the clips in view plus a margin of about one view's width each side, and the ruler's bar numbers the same; tracks more than a view above or below draw no clips but keep their row boxes. Notes draws only the notes in view plus a margin, across and down. The window moves in steps of half a view, never every frame. The beat grid keeps its culling by bar, through the camera. The Graph is small: no culling. | §8, VIEW-12 |
| 4 | **A clip's mini drawing doesn't render on a zoom.** The notes, the curve and the steps in a clip don't read the zoom and are memoised by their clip: a zoom changes only the clip's box. | §8 |
| 5 | **The numbers, until the owner's probe report says otherwise** (it did, 2026-11-14: item 10): G = 50 ms; rails within 20° after 8 px (10°, and breakable, since item 9: by a turn of the last 50 px over 20°, since review Q); the system's glide on the Mac, the camera's own on WebKitGTK (0.998 a ms); the browser's own scrollbars (D5); edges stop (D3). Each is a named constant in `web/src/camera/numbers.ts`, so the report changes one line. | §5, §6 |
| 6 | **120 Hz is done** (track `mac-feel`, VIEW-11): track 1 leaves §13. | §8, §13 |
| 7 | **The probe and the camera.** While the swipe probe listens, it pauses the camera's swipes (`pauseSwipes`): the web view scrolls a plain swipe, which the camera takes in, and the camera still zooms. So round A measures what WebKit does on its own, as before the camera, and round B's listener, on the window and first, cancels and moves each event itself, as before. The probe measures the platform the camera's numbers come from; the camera moving the view too would change what it measures. | §5, VIEW-10 |
| 8 | **The recognizer owns a whole swipe from its first event** (the owner's "unnatural" pan on #112, where two owners moved one swipe). Every event of a swipe the camera owns is the camera's, whatever its deltas; only an event WebKit marks as not cancelable is the web view's, and its move is taken in. | §5, VIEW-2 |
| 9 | **Rails: none in the Graph, and breakable elsewhere** (the owner's answer, 2026-11-12, after "diagonal stopped working" and "use best practice"). A canvas pans freely 1:1, as Figma, Freeform and tldraw do: rails are a camera's option, off for the Graph. The arrangement, Notes and the beat grid keep rails, made breakable as Chromium's touch scroll rails are: a swipe whose first 8 px lie within 10° of an axis (was 20°) starts on that rail, and the rail breaks, for the rest of the swipe, once it has gone 24 px off it (`RAIL_BREAK`); what was held back off the rail isn't moved when it breaks. Since review Q (Q3): a lifetime 24 px broke long nearly straight swipes and their glides, so it breaks on a turn instead: the last 50 px (`RAIL_RECENT`) more than 20° (`RAIL_TURN`) off the rail, with signs, and the start's angle is from signed sums too. | §5, VIEW-3 |
| 10 | **The probe's numbers** (the owner's report, 2026-11-14: their Mac, its 120 Hz screen, in the app). **G stays 50 ms:** a flick's first tiny event often comes 50 to 65 ms before its next, so G splits it, harmlessly (both parts' first events decide the same owner); 80 or 120 ms joins swipes, and the rails get worse (5 px off instead of none). **The rails: within 25° after 16 px, broken by the last 100 px turning 25° off** (were 10°, 8 px, 50 px and 20°; the break's angle is the start's, so a swipe that may start on a rail at 22° also stays on it at 22°): the owner's straight swipes go 0 to 11° off all told, but their first 8 to 20 px often 15 to 25° off, in small whole deltas such as (1, 0), (3, 0), (5, −3). The old numbers put 14 of their 32 straight swipes on a rail, and moved them 328 px of the 681 they went off it; the new ones all 32, and none. None of their 11 diagonal swipes (25 to 45°) gets a rail, and 98% of their move across is kept (4721 of 4814 px). One flick only 19° off all told, in a step of diagonal flicks, stays on its rail (8 of its 333 px across move). Their swipes over the Graph count too, as more of the same hand. **The glide:** the Mac's reaches the page and stays cancelable (round B cancelled all 3121 events, glides included; WebKit took over none; the page heard some events up to 62 ms late). **Frames:** a median of 8 ms, 125 a second (VIEW-11 works). **Scrollbars (D5):** shown in every step, round B too, but that Mac shows them always, so the overlay case is still not measured. Still guesses: the overlay scrollbars, the culling's margin, and the pinch's cost on the tracks (its timeline pinch had 69 frames over 16.7 ms), in gaps.md. | §5, §6, VIEW-2, VIEW-3, VIEW-4, VIEW-7 |

