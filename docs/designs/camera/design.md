# The camera: views that move as the hand does

| | |
|---|---|
| **Status** | Proposed, 2026-11-11. Waiting for the skeptic's review (§15) and the owner's yes (§14). |
| **Written** | 2026-11-11, from `main` at `18e52c44`. The owner asked for it in these words: "the scrolling and panning doesn't feel very smooth on macOS, for both the tracks and devices; can you make sure they feel as native as possible", "it doesn't scroll diagonally", "I want React to still be like the source of truth, and so that I don't have to deal with like native apps, and also might benefit on Linux platforms", "if I would re-architect the scrolling UI from first principles, would you go with the camera option?", and "go ahead and do it then, using best practices". |
| **Replaces** | PR #112 (a diagonal swipe scrolled by `web/src/lib/freeScroll.ts`), which closes unmerged; the browser's own scrolling in the four views; TL-16's scroll padding. §11 maps every rule. |

**In short**
- **Four views get a camera:** the arrangement, the Graph, Notes and the beat grid. A camera is three numbers, where the view looks (x and y) and its zoom, kept in the app's state. It is the one truth: what each view shows follows from it.
- **Every swipe is the camera's,** from its first event to its last, in any direction, diagonal included. The browser scrolls none of them. So a swipe moves as the fingers do, the same on the Mac and on Linux.
- **The camera draws by the box's scroll position,** as VS Code's editor does, not by moving layers. Sticky headers, Tab, "scroll into view", the waveforms that draw when seen, and every browser suite keep working. A move by the browser itself (focus, Tab, a test's click) is taken back into the camera.
- **It feels like the Mac:**
  - 1:1 with the fingers, written once a frame;
  - the system's glide after a fast swipe;
  - pinch and ⌘-scroll zoom around the pointer in all four views;
  - a bounce at the edges;
  - thin scrollbars that show while you move and fade after.
- **Moving the view redraws nothing.** No React component renders while the camera moves; the playhead's polls stop redrawing these views; a zoom redraws at most once a frame. A frame meter shows it on the owner's Mac.
- **The core gains nothing.** Rust changes only if the owner says yes to 120 Hz on the Mac (D4): one switch in the Mac shell.

## 1. What is wrong today

- **A swipe moves one way only.** WebKit, the web view the app runs in on the Mac, passes every swipe through AppKit's "predominant axis" filter, which keeps only the way the swipe mostly goes (`WheelEventDeltaFilterMac.mm`). WebKit has no setting to turn it off. Native views that move freely turn it off for themselves (`NSScrollView.usesPredominantAxisScrolling`); a web page can't. WebKit on Linux uses a simpler filter of its own (`BasicWheelEventDeltaFilter`).
- **PR #112's patch can't be made smooth.** It moved the view itself only for the events of a swipe that went both ways, and left the rest to the browser, so one swipe flipped between two engines. Worse, WebKit lets a page cancel a swipe only at its first event (`WheelEventGesturesBecomeNonBlocking`): a swipe that started straight and turned diagonal was moved by both, twice as far one way.
- **The views fight their scroll boxes.** The arrangement keeps a `spare` width and a `flushSync` so that a zoom has room to scroll to (`SongTimeline.tsx:473-482`), an `ours` and an `across` to tell its own scrolls from a hand's (`430-467`), and reads `scrollLeft` in eleven places. The Graph measures room for scrollbars that come and go (`NodeGraph.tsx:166-181`, `266-273`), though Fit no longer uses it (`281-283`). Notes and the beat grid copy `scrollLeft` into their velocity strips by hand (`NoteView.tsx:1239`, `BeatView.tsx:353-356`).
- **Zoom is uneven.** The arrangement's pinch renders the whole timeline on every gesture event, through `flushSync`. The Graph's pinch and ⌘-scroll don't zoom around the pointer (`NodeGraph.tsx:202-215`). Notes and the beat grid have no pinch.
- **The window redraws while it plays.** The status poll (every 200 ms) hands the arrangement a new `beat`, so its top renders five times a second while a song plays, though the playhead already moves by itself each frame (TL-9).
- **The Mac's 60 Hz.** WKWebView in an app (not Safari) draws at most 60 frames a second on macOS 13 to 15, also on a 120 Hz screen, while native apps draw 120 ([WebKit bug 294338](https://bugs.webkit.org/show_bug.cgi?id=294338)). Reports differ on macOS 26.

## 2. How others do it

| Who | How the view moves | What we take |
|---|---|---|
| Safari, Kinopio | the browser's own scrolling | It is the smoothest, but every swipe keeps to one way. Not enough for a DAW. |
| VS Code's editor (Monaco, already in our code view) | its own scroll position; every wheel event is its own; a classifier tells a trackpad from a wheel; overlay scrollbars that fade after 500 ms; `DomScrollableElement` writes its position back into the element's `scrollLeft` and `scrollTop` | Nearly all of it: the classifier, the overlay bars, and drawing by the box's scroll position. |
| tldraw, Excalidraw, Figma | a camera in the app's state; every wheel and pinch is theirs; Safari's gesture events for a pinch on the Mac, Ctrl + wheel elsewhere; draw only what is on screen; no hover checks while the camera moves | The camera, the pinch, and "moving costs nothing". Their layers moved by transforms suit an endless canvas with no sticky rows, which our views are not (§4). |
| `wheel-gestures` (Embla), `@use-gesture` | a recognizer: a swipe's start and end, its glide, its speed | The idea, in a few lines of our own: no new dependency. |
| Apple's scroll views | 1:1 with the fingers, the system's glide, a rubber-band at the edges (`f(x) = (1 − 1/(x·0.55/d + 1))·d`) and a spring back | The feel to match. |

## 3. The camera

- **What it is.** Each of the four views has one camera: `{ x, y, zoom }`.
  - `x` and `y` are the top-left of what shows, in the content's pixels at the current zoom, as `scrollLeft` and `scrollTop` are.
  - `zoom` is the view's own unit: pixels a beat in the arrangement, Notes and the beat grid; the scale in the Graph.
  - The arrangement's zoom stays in `view.json` (FMT-20), as today. Where the view looks isn't saved, as today.
- **Where it lives.** One small store, `web/src/lib/camera.ts`, in the same style as the app's other stores (zustand), with one camera per open view.
- **What it knows.**
  - The view's size, from a ResizeObserver: never read while moving.
  - The content's size, from the view: the song's room in the arrangement (`room()`), 128 rows in Notes, the layout in the Graph.
  - Its edges, the fixed parts that cover it (the ruler, the track names, the Song row, the keys), so a move that reveals something puts it clear of them.
- **What it does,** as pure functions, each tested on its own:
  - `move(dx, dy)` and `to(x, y)`: clamped to the content;
  - `zoomAround(point, zoom)`: what is under the point stays there;
  - `reveal(box, margins)`: the least move that shows a box;
  - `fit(range)`: a range fills the view (Z, Fit);
  - `toContent(clientX, clientY)` and back: the one conversion every gesture uses.
- **Who moves it:**
  - input (§5);
  - the app's own moves (§7): Follow, Fit, Z, Show, a marker, a new lane, an expanded node, a lit column;
  - the browser, when it scrolls the box itself (§4).

  Each move says which of the three it is, so Follow knows a hand from itself without guessing.

## 4. Drawing: by the box's scroll position

- **The box stays a scroll box**, with its scrollbars hidden. The browser never scrolls it on its own, because every wheel and trackpad event over it is cancelled from its first (§5).
- **Once a frame, the camera writes the box.** It writes its size first, if it changed, and then its `scrollLeft` and `scrollTop`, in one place (`useCamera`), and reads nothing. The content is never narrower than where the camera looks. That replaces the arrangement's `spare` and `flushSync`.
- **What the browser moves is taken back in.**
  - It may still move the box itself: a focused bar brought into view by Tab (DV-15), a rename field's autofocus, the arrow and page keys on the focused timeline, a test's click.
  - Its `scroll` event, when the position isn't the one the camera wrote, sets the camera to it. So the camera stays the one truth.
- **Why not move layers by transforms,** as tldraw does? [The inventory](inventory.md) behind this note found what depends on a real scroll box:
  - the sticky ruler, names, keys, Song row and tools, in four views;
  - IntersectionObservers that draw waveforms (`Wave.tsx:167`) and ask for scopes only while seen (TAP-16);
  - every `scrollIntoView`;
  - Tab's focus;
  - Puppeteer's clicks, which scroll the box before they press, in about forty browser suites.

  With transforms, every one of those is rebuilt or rewritten, and a box with `overflow: hidden` still moves under the transform when focus or a click scrolls it. Setting a scroll box's position from script costs, like setting a transform, no layout and no repaint of what is drawn: the compositor moves it. So transforms buy nothing here unless the frame meter shows otherwise (§8).
- **Overscroll at an edge** (the Mac's bounce, VIEW-6) is the one transform: the content moves by the stretch, and springs back.
- **Small guards:**
  - `overflow-anchor: none`, so the browser never shifts the box when rows above change height;
  - `overscroll-behavior: none`, so nothing reaches the window.

## 5. Input: one recognizer

One wheel listener per view, on the view, not passive. It decides once, at a swipe's first event, and keeps to it to the last.

- **Whose event it is.**
  - It is the camera's, and cancelled, unless one of these holds:
    - ⌘ or Ctrl is down: that's a zoom;
    - a control under the pointer took it already: a picture's dot (DV-17), a Mix knob;
    - a list inside the view can scroll that way: a menu, the effect browser.
  - It is cancelled from the first event, so WebKit's first-event rule keeps the whole swipe ours.
- **Trackpad or wheel.** Monaco's classifier scores the last five events:
  - a delta both ways, or a fraction of a pixel, means a trackpad;
  - equal steps that repeat mean a wheel.
- **A trackpad** moves the camera exactly as far as the event says, both ways at once. The system's acceleration is already in it.
- **A wheel** moves one notch as the system says, at once (as VS Code does by default). ⇧ turns it across where the system doesn't already (Linux).
- **Once a frame.** The events of one frame are summed and written once, from `requestAnimationFrame`. Nothing is read from the page on the way.
- **The glide.**
  - On a Mac, the system's glide arrives as more wheel events, both ways (WebKit's `MomentumEventDispatcher` builds it from the raw deltas). The camera moves by them like any other. Track 1 confirms on the owner's Mac that they still arrive when the swipe was cancelled; the frame meter logs them.
  - Where a fast swipe stops sending events with no glide after it (60 ms of silence while it moved faster than 0.5 px a millisecond), the camera glides itself. It starts at the swipe's speed and slows as Apple's scroll views do (their normal rate, 0.998 a millisecond).
  - A new touch, a click, or a key stops either glide.
  - Whether WebKit on Linux sends a glide is measured on a Linux laptop. Until then this rule covers both cases, and no swipe glides twice: a glide's own events keep the silence from starting.
- **Zoom.** Pinch (Safari's gesture events on a Mac, Ctrl + wheel elsewhere) and ⌘ + wheel zoom around the pointer, in all four views: the arrangement and Notes in time, the Graph in scale. The beat grid zooms in time, as Notes does.
  - The keys stay as they are: ⌘= and ⌘− around the playhead or the middle; Z and Z again (TL-11, Q9).
  - A zoom is written at most once a frame. A pinch's last scale is kept when the fingers lift.
- **Edges.**
  - **On a Mac:** past an edge, the content stretches by Apple's curve (`c = 0.55`, `d` the view's size that way) and springs back when the swipe or its glide ends, in about 300 ms, critically damped. The glide that reaches an edge bounces the same way. Reduced motion means no stretch: the view stops.
  - **On Linux:** the view stops at the edge, as GTK's views do.

## 6. Scrollbars

- **Thin bars over the view,** as on a Mac:
  - 6 px, growing to 10 px under the pointer, in `fg-faint`;
  - shown while the camera moves or the pointer is over them, and gone 500 ms after (VS Code's time);
  - each takes no room.
- **They are buttons and drags:** drag the thumb to move; click the track to move a page; the thumb's size is the view's share of the content.
- **The browser's own bars are hidden** in the four views (`scrollbar-width: none`, and `::-webkit-scrollbar` for WebKit). Lists elsewhere keep theirs (`index.css:124`).

## 7. The app's own moves

Every move the app makes goes through the camera. None calls `scrollIntoView` or writes `scrollLeft`.

| Move | Today | With the camera |
|---|---|---|
| Follow (Q11) | pages at 85% to 15%; a hand's scroll pauses it, guessed by `ours`/`across` | the same pages; a move from input pauses it, known by its source |
| ⌘=, ⌘−, pinch, ⌘-scroll (TL-11, Q9) | `zoomScroll`, `flushSync`, `spare` | `zoomAround` |
| Z, Fit (TL-11, DV-5) | `zoomFit`, `fitted` | `fit`, and Z back to the camera before |
| Show (AGENT-6) | `showBeats`; `scrollIntoView` for a lane, clip, track or knob | `reveal`; for bars, from 15% in when they fit, as today |
| Add Marker | scrolls the bar to 15% | `reveal` with the same margin |
| A new lane or track (TL-16) | `scrollIntoView` with `scroll-padding` | `reveal` with the ruler, the names and the Song row as margins |
| An expanded node (DV-13), a lit column (DV-18) | `scrollIntoView` after the move | `reveal` after the move |
| Notes opens | centres the clip's pitches | `to(…)`, as today |

- **What agents see** (AGENT-6's "the bars the timeline shows") is read from the camera, rounded as today (`focus.ts`), and not from a scroll event.
- **The pointer's note** (`PointerNote.tsx`) follows the camera, not every frame.

## 8. Speed

- **Moving the camera renders nothing.**
  - A move writes two numbers to the DOM. No React component renders: not the view, not its rows, not its clips.
  - Notes' and the beat grid's velocity strips, the beat grid's "near" steps, and agents' `seen` read the camera by subscription.
  - The beat grid's steps redraw when the camera crosses a bar, not every frame (KIT-10's "draws only the steps near what shows").
- **The status poll doesn't render the views.** The arrangement takes no `beat` prop: Follow and the room after the playhead read the clock (TL-9's `onFrame`), and the room grows a bar at a time, as today.
- **A zoom renders at most once a frame.**
  - If one render of the arrangement at its biggest example song takes over 8 ms, positions move to a CSS variable for the zoom (`--ppb`), so a zoom becomes one style write. That is measured first, not built ahead.
- **The frame meter.**
  - "Show frame rate" in the command palette shows, over the window:
    - the frames a second;
    - the slowest frame of the last second;
    - how many React renders the last swipe caused.
  - Suites read the same numbers from `canvas.frames()`.
  - It is how the owner's Mac says whether this is smooth, since no machine here has a trackpad.
- **120 Hz on the Mac** (D4, if yes):
  - The Mac shell turns off WebKit's `PreferPageRenderingUpdatesNear60FPSEnabled` when it makes the window, through `WKPreferences`' private `_features` API, as `tauri-plugin-macos-fps` does.
  - That is fine for a downloaded app, and the Mac App Store would refuse it.
  - If the API is gone, the shell logs one line and keeps 60 Hz.
  - It lives in `apps/music2-desktop` (PLAT-2's shell exception). Its Objective-C binding is a dependency, in its own pull request first (AGENTS.md rule 7).

## 9. Tests

- **The camera's functions** (`move`, `zoomAround`, `reveal`, `fit`, the classifier, the glide, the stretch) are pure, and each has its unit checks. They run in a browser suite, since `web/` has no unit runner.
- **A browser suite, `camera`, written first.** Wheel events are now the camera's in Chrome too, so a suite's synthetic swipes test the real path. It checks, in the arrangement and the Graph:
  1. diagonal, straight, and a swipe that turns: each moves exactly as far as its events, never twice;
  2. a wheel's notch against a trackpad's pixels;
  3. the glide after a fast stop, none after a slow stop, and a click stopping it;
  4. edges: a stop off the Mac, and a stretch and spring back with the Mac's flag set in the page;
  5. pinch and ⌘-scroll keep the beat, or the node, under the pointer;
  6. 120 events of a swipe cause no React render;
  7. a swipe while playing pauses Follow;
  8. agents' `seen` follows the camera;
  9. the scrollbar's drag and page click;
  10. a Tab or a test's click that scrolls the box is taken into the camera.
- **The suites that read or write `scrollLeft`** keep working, since the box still scrolls and its moves are taken in (pl-zoom, layout, ag-focus, notes, ports and the rest: §6 of [the inventory](inventory.md)).
- **One check changes meaning.** `ports`' "Fit with real scrollbars" (`ports.mjs:272-358`) checked room left for bars that now take none. It becomes "the bars take no room".
- **By hand, on the owner's Mac,** after each track: a checklist.
  - Swipe straight, diagonal, and turning.
  - Flick, and stop a glide.
  - Hit each edge.
  - Pinch over a clip and over a node.
  - Drag a scrollbar.
  - Read the frame meter while a song plays.

## 10. What goes

- PR #112: `freeScroll.ts` and its `scroll` suite (the suite's checks move into `camera`).
- In the arrangement:
  - `spare`, `zoomScroll`'s `flushSync`, `ours` and `across`;
  - the `scroll` listener for `seen`;
  - the `beat` prop;
  - the `scroll-padding` (TL-16).
- In the Graph: `scrollbarRoom()` and `room0`, which Fit no longer reads.
- In Notes and the beat grid: the `onScroll` mirrors and the beat grid's `left` state.
- Every `scrollIntoView` in the app's own code: `SongTimeline.tsx:1746`, `pointer.ts:138`, `NodeGraph.tsx:1241`, `PictureView.tsx:332`.

## 11. The rules: kept, rewritten or added

The contract changes first, in track 1's first commit. The new rules are VIEW-1 to VIEW-10, in taste.md's new §5.4, "Moving the view".

| Rule | Becomes |
|---|---|
| **VIEW-1** (new) | One camera per view (the arrangement, the Graph, Notes, the beat grid): where it looks and its zoom, the one truth of what it shows (§3). |
| **VIEW-2** (new) | Every wheel and trackpad event over these views is the camera's, from a swipe's first event to its last, in any direction; the browser scrolls none. The exceptions are §5's. |
| **VIEW-3** (new) | 1:1 with the fingers, a wheel's notch as the system says, written once a frame. |
| **VIEW-4** (new) | The glide (§5). |
| **VIEW-5** (new) | Pinch and ⌘-scroll zoom around the pointer in all four views. |
| **VIEW-6** (new) | Edges: a stretch and spring back on a Mac, a stop elsewhere; none with reduced motion. |
| **VIEW-7** (new) | The scrollbars (§6). |
| **VIEW-8** (new) | Moving the camera renders no React component; the status poll doesn't render these views; a zoom renders at most once a frame. |
| **VIEW-9** (new) | The app moves these views only through the camera (§7); a move by the browser itself is taken into it. |
| **VIEW-10** (new) | The frame meter. |
| taste.md §2.13 (no motion for zoom and scroll) | **Rewritten:** no motion except VIEW-6's stretch on a Mac. |
| taste.md §5.2 (gestures) | **Rewritten:** rows for a swipe in any direction, a pinch, ⌘-scroll and the scrollbars. |
| TL-9, TL-11, Q8, Q9, Q11, Q31, X25, X33 | **Kept.** |
| TL-16 (scroll padding, `block: "nearest"`) | **Rewritten:** `reveal` with the ruler, the names and the Song row as margins. |
| DV-5 ("a graph bigger than the dock scrolls"), DV-14 (the wheel scrolls the graph) | **Kept**, through the camera. |
| DV-13, DV-16, DV-18, AGENT-6 (scroll a node, a value, a column into view; Show) | **Kept**, through `reveal`; their *Today* lines name it. |
| DV-10, TAP-4, TAP-8, TAP-16 (ask only while seen) | **Kept**: the box still clips, so the IntersectionObservers still see. |
| KIT-10 (the beat grid scrolls and zooms as Notes does) | **Kept**, with a pinch as Notes now has; its culling reads the camera. |
| NOTE-8, NOTE-9, NT-5 (rows and their height) | **Kept.** |
| FMT-20 (`view.json`: zoom, follow) | **Kept.** |
| PLAT-2 | **Kept:** the 120 Hz switch is in the shell, which PLAT-2 allows. |

## 12. What the core gains

Nothing in the engine, the editor, the server or the file formats. In `web/`:
- `lib/camera.ts`: the store, its functions, the recognizer;
- `useCamera`: the writer;
- one scrollbar component;
- the frame meter.

In Rust, only D4's switch, if yes.

## 13. Build order

Each track has a builder, a skeptic and the builder again, and each changes the contract first.

| # | Track | Builds | Size |
|---|---|---|---|
| 1 | `camera-core` | The contract (§11). `lib/camera.ts`, `useCamera`, the recognizer, the scrollbars, the frame meter. The arrangement on its camera: zoom, Fit, Z, Follow, Show, markers, `seen`, TL-16's reveal; the `beat` prop gone. The `camera` suite. PR #112 closed. | large |
| 2 | `camera-views` | The Graph, Notes and the beat grid on their cameras: zoom around the pointer, pinch in the three, DV-13 and DV-18's reveals, the strips and the culling by subscription. | medium |
| 3 | `mac-120hz` | D4, if yes: the dependency's own pull request, then the shell's switch. It runs beside track 1. | small |

After track 1, the owner tries it on the Mac with the frame meter on. Track 2 starts after that answer, so a wrong feel is fixed once, in one view.

**Not in this note:**
- Drawing only what is on screen in the arrangement and Notes: built only if the meter shows a long frame at the biggest example, with a note of its own.
- Drag-to-scroll at the edges during a clip drag (none today).
- The Mix strips, which scroll one way and keep the browser's scrolling.
- The code view, which is Monaco's.
- A Follow that scrolls smoothly instead of paging.

## 14. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | The four views get a camera, and every swipe over them is the camera's. All right? | **Yes.** It is the owner's own choice (2026-11-11). |
| D2 | The camera draws by the box's scroll position (VS Code's way), not by moving layers. All right? | **Yes.** Sticky rows, Tab, waveforms and forty suites keep working, and it moves on the compositor all the same. Layers only if the meter shows a need. |
| D3 | Edges: a Mac stretches and springs back; Linux stops. All right? | **Yes.** Each feels like its own system. |
| D4 | 120 Hz on the Mac, through WebKit's hidden setting. All right? It is safe outside the Mac App Store; Apple may change it in a later macOS, and then the app keeps 60 Hz. Which Mac, and which macOS, do you test on? | **Yes,** if your Mac has a 120 Hz screen (a MacBook Pro from 2021 on). |
| D5 | Thin scrollbars of our own in the four views, showing while you move. All right? | **Yes.** |
| D6 | PR #112 closes unmerged. All right? | **Yes.** It is replaced whole. |

## 15. The review, answered

To come.
