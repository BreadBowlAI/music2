# What depends on the scroll boxes today

**In short:** everything in the arrangement, the Graph, Notes and the beat grid that reads, writes or relies on their scroll boxes, from `main` at `18e52c44` (2026-11-11), with `file:line`. It is the ground the camera note ([design.md](design.md)) stands on. Line numbers are as of that commit.


**Files referenced.** Short names below map to these absolute paths:
- **Components** (`web/src/components/`): `SongTimeline.tsx`, `NodeGraph.tsx`, `NoteView.tsx`, `BeatView.tsx`, `TakeRows.tsx`, `HiddenLanes.tsx`, `Markers.tsx`, `Wave.tsx`, `PictureView.tsx`, `CardViews.tsx`, `EffectBrowser.tsx`, `Knob.tsx`, `Bar.tsx`, `ui.tsx` (Popover), `ClipMenu.tsx`, `PointerNote.tsx`, `DropLayer.tsx`, `KeyboardPiano.tsx`, `Tracks.tsx`, `TrackName.tsx`, `Dock.tsx`
- **lib** (`web/src/lib/`): `zoom.ts`, `focus.ts`, `pointer.ts`, `heights.ts`, `drop.ts`, `taps.ts`, `shell.ts`, `grid.ts`, `local.ts`, `expanded.ts`
- **App:** `web/src/App.tsx`
- **Docs:** `docs/architecture.md`, `docs/designs/step8/taste.md`, `docs/designs/step8/polish-plan.md`
- **Browser suites:** `e2e/*.mjs` (driver is puppeteer-core 25.16.0)

**Checked with no hits** (no scroll, zoom-geometry or "seen" code): `lib/heights.ts`, `lib/overlays.ts`, `lib/clips.ts`, `lib/sections.ts`, `lib/markers.ts`, `lib/points.ts`, `lib/steps.ts`. `heights.ts` only changes row heights, which changes scrollHeight indirectly.

---

## 1. Scroll reads and writes, listeners, sticky, scroll-padding, client→beat conversions

### Arrangement (`SongTimeline.tsx`, scroller `ref={scroller}` at 1814, `data-testid="timeline"`)

**The scroller element**
- 1813–1823: the scroller is `overflow-auto relative`, `tabIndex={-1}` (1817), has `onKeyDown` (1818) and `onScroll={onScroll}` (1819).
- 1822: `scrollPaddingTop: RULER_H, scrollPaddingLeft: headerW, scrollPaddingBottom: songH`, for TL-16 (comment at 1821).
- 1824: the content div is `width: headerW + width, minHeight: 100%`.

**Measuring the view**
- 410–415: a ResizeObserver sets `visible = clientWidth − headerW`.

**Programmatic scrolling**
- 433–438: `scrollTo(left)` writes `el.scrollLeft` and records `ours = el.scrollLeft`.
- 447–457: follow, on every frame, reads `clientWidth` and `scrollLeft`, then calls `scrollTo`.
- 460–467: `onScroll` reads `scrollLeft` against `across`/`ours`, so a scroll by hand sets `paused`.

**Zoom**
- 473–482: `zoomScroll` reads `clientWidth`, then `flushSync(setZoom, setSpare)`, then `scrollTo`.
- 483–490: `zoomTo` reads `scrollLeft` to keep the beat under `at`.
- 492–498: `zoomStep` reads `clientWidth` and `scrollLeft` (playhead in view?).
- 500–518: `zoomFit` reads `clientWidth` and `scrollLeft` and saves `{zoom, scroll}` for Z-back.

**Markers, pointers and the focus record**
- 547–552: `marker("add")` reads `clientWidth` and `scrollLeft` and scrolls the bar to 15%.
- 556–559: `beatX(b)` = `getBoundingClientRect().left + headerW + b*zoom − scrollLeft` (pointer at bars).
- 561–567: `lanesBox()` uses the rect plus `clientWidth`/`clientHeight` minus RULER_H and the Song row height (`songRow` ref, 432 and 1809).
- 569–575: `showBeats` reads `clientWidth` and calls `scrollTo` (AGENT-6 Show).
- 583–590: `el.addEventListener("scroll", say)`, where `say` calls `look({ seen: [scrollLeft/zoom, (scrollLeft+clientWidth−headerW)/zoom] })`. It re-subscribes on `[ppb, visible]`.

**Wheel and pinch**
- 595: `at(x) = x − rect.left − headerW` (wheel and pinch anchor). There is no scrollLeft term; it is added in `zoomTo`.

**Gestures that convert client x/y**
- 863–865: marquee `beatAt = (x − content.rect.left − headerW)/zoom`. `content` moves with the scroll, so scrollLeft is implicit.
- 869–871: the marquee reads lane rows' client rects.
- 881–886: the marquee box is stored in content pixels (`y − c.top`).
- 1000–1003 and 1084–1087: step and automation-point drags. `beatAt` uses the clip rect `r` captured at press (1218–1219, 1234–1235). A scroll during the drag is not compensated.
- 1225 and 1239–1242: hit-testing points against the clip rect.
- 1279: clip move/resize uses `raw = (ev.clientX − x0)/zoom`, a delta. A scroll mid-drag is not compensated.
- 233 and 1302: `laneUnder` = `elementFromPoint(...).closest("[data-lane]")` (the drop lane while dragging).

**Focus that must not scroll**
- 996, 1075, 1254, 1540: `scroller.focus({ preventScroll: true })`.
- 2585: the Notes root `focus({ preventScroll: true })`.

**Reveal**
- 1742–1746: `reveal()` finds the `[data-lane]` row and calls `row.scrollIntoView({ block: "nearest" })`. It relies on the scroll-padding at 1822.

**Sticky elements**
- 1825: the ruler row is `sticky top-0 z-30`.
- 1827: the corner is `sticky left-0 z-40`.
- 1931: the Song row is `sticky bottom-0 z-[26]`.
- 1974: ViewTools are `sticky right-0 z-40` with `marginLeft: -TOOLS_W` (1975).
- 2378: the filler row's header is `sticky left-0`.
- 2383: the empty state is `sticky`, `left: headerW, width: visible`.
- 2517: each row header is `sticky left-0 z-20`.
- 2567: the bus words are `sticky left-0`.

**Ruler, section band and lanes**
- 2148–2149: ruler right-click beat = `(clientX − ruler.rect.left)/ppb`.
- 2154–2155: ruler seek/loop drag `beatAt` uses the ruler rect left taken at press.
- 2287–2288: SectionBand `at(x)` uses the band rect left taken at press.
- 2553, 2556, 2561: lane place, create and menu beat = `(clientX − lane.rect.left)/ppb`.
- 2588: clip menu beat from `parentElement.rect.left`.

**Children of the timeline**
- `TakeRows.tsx:108–111`: take swipe beat from the row rect captured at press.
- `TakeRows.tsx:160`: `sticky left-0` header.
- `HiddenLanes.tsx:33`: `sticky left-0` header.
- `Markers.tsx:113–118`: marker drag by delta `(clientX − x0)/ppb`.
- `Wave.tsx:280`: fade handle uses the parent rect captured at press.
- `Tracks.tsx:347–371` (`startReorder`): row midpoints from rects at press, `dy` from clientY. No scroll compensation.

**lib helpers and other consumers**
- `lib/pointer.ts:83–103` (`targetBox`): rects of header, row, clip and knob; bars via `canvas.beatX` (100).
- `lib/pointer.ts:117–127` (`seenIn`): `canvas.lanesBox()` and the timeline rect.
- `lib/pointer.ts:135–162` (`reveal`): `requestAnimationFrame(() => el.scrollIntoView({ block: "nearest", inline }))` (138); bars via `canvas.reveal` (141); clips with `inline: "center"` (160).
- `PointerNote.tsx:92–108`: a rAF loop re-places the note every frame "as the view scrolls, zooms or resizes".
- `lib/drop.ts:77–101` (`target`): beat = `(x − ruler.rect.left)/zoomNow()` (85–87); bounds from the ruler and timeline rects; the Song row rect (88); `elementFromPoint` (78, 90).
- `DropLayer.tsx:13–38`: outline left = `ruler.rect.left + beat*ppb` (24), plus row and track rects.
- `KeyboardPiano.tsx:24–34`: a ResizeObserver on `[data-testid=timeline]` places its card at the timeline's corner.
- `lib/focus.ts:36–38, 69`: the `seen` field of the focus record, rounded to 1/1000 "so a scroll that moves nothing says nothing" (48).

### Graph (`NodeGraph.tsx`, scroller at 164 and 230, `data-testid="graph"`)

**The scroller and its measurements**
- 230: the scroller is `flex-1 min-h-0 overflow-auto p-4 flex flex-col`.
- 166–181: a layout effect plus ResizeObserver measures `offsetWidth/offsetHeight − scrollbarRoom()` into `room0`.
- 266–273: `scrollbarRoom()` probes `overflow:scroll` and returns `offsetWidth − clientWidth`.

**Zoomed content and conversions**
- 830, 834, 838: the canvas is `minWidth/minHeight: width*zoom`, an inner box `width*zoom × height*zoom`, and `area` with `transform: scale(zoom)` and `origin-top-left` (837).
- 746 and 757–763: cable drag converts client to graph coordinates as `(clientX − area.rect.left)/zoom`. The rect is re-read on every move, so a scroll mid-drag is handled.
- 752 and 776: `elementFromPoint` finds the drop target.

**Programmatic scrolling**
- 1237–1246: DV-13. 290 ms after expanding, `el.scrollIntoView({ block: "nearest", inline: "nearest" })`.
- `PictureView.tsx:323–333`: DV-18. 320 ms after `light()`, `[data-column]` `scrollIntoView({ block: "nearest", inline: "nearest" })`.
- `lib/pointer.ts:143–153`: knob Show calls `expandFor`, then `scrollIntoView` (DV-16 / AGENT-6).

**Wheel handlers inside the graph**
- `Knob.tsx:495–513`: the knob wheel `preventDefault`s so "the canvas under it doesn't scroll". It passes ⌘/Ctrl through to the canvas zoom.
- `PictureView.tsx:533–550`: the dot wheel `preventDefault`s, so it never scrolls the graph.
- `Bar.tsx:60`: comment only; bars deliberately let the wheel scroll the graph (DV-14).

**Geometry that includes the scale**
- `PictureView.tsx:175–189` (`useSize`): `getBoundingClientRect` plus a ResizeObserver. The on-screen size includes the graph's `scale()`.
- `PictureView.tsx:396`: play rects.
- `CardViews.tsx:186`: `svgPoint` uses the svg rect.

**Sticky:** none in the Graph. No `onScroll` and no scroll-padding.

### Piano roll (`NoteView.tsx`, `scroller` at 173, `data-testid="roll-scroller"` at 1236–1240)

**The scroller**
- 1238: `absolute inset-0 overflow-auto`.
- 1239: `onScroll` mirrors `scrollLeft` into the velocity strip's `translateX`.
- 1405: the strip's initial transform reads `scroller.current?.scrollLeft` during render.

**Opening**
- 252–265: the layout effect sets `ppb` from `clientWidth` (255), computes `centred` from `clientHeight` (261), and writes `el.scrollTop` (262). Only on open.

**Measuring and zoom**
- 267–270: a ResizeObserver sets `visible = clientWidth − keysW`.
- 272–279: ⌘-wheel reads the rect left (275) and `scrollLeft` (277) into `anchor`.
- 287–291: a layout effect on `[ppb]` writes `el.scrollLeft = anchor.beat*ppb − anchor.x`.
- 292–296: `zoom(by)` reads `scrollLeft` and `clientWidth` into `anchor`.

**Row height**
- 298–306: `resizeRows` reads `scrollTop` and `clientHeight` into `rowAnchor`.
- 307–311: a layout effect on `[rowH]` writes `el.scrollTop`.

**Client → beat/pitch**
- 532–536: `at(e)` = `grid.rect` gives `beat = x/ppb` and `pitch = row(y/rowH)`. The grid rect moves with the scroll, so this is scroll-safe per event. Used by every grid gesture (823, 874, 880, 1274).
- 1048–1050: velocity from the strip rect.
- 1695: `Keys.pitchAt` from `keys.rect.top`.
- 1866: ruler click beat from the ruler rect.

**Focus:** 263, 821, 1037, 1065 call `root.focus({ preventScroll: true })`.

**Sticky**
- 1242: the ruler row is `sticky top-0 z-30`.
- 1243: the corner is `sticky left-0 z-40`.
- 1704: the Keys column is `sticky left-0 z-20`.

### Beat grid (`BeatView.tsx`, `scroller` at 148, `data-testid="beat-scroller"` at 350–356)

**The scroller**
- 352: `absolute inset-0 overflow-auto`.
- 353–356: `onScroll` mirrors `translateX` into the strip and calls `setLeft(scrollLeft)`, which drives culling.
- 437: the strip's initial transform reads `scroller.current?.scrollLeft` during render.

**Measuring and zoom**
- 185–187: a ResizeObserver sets `visible = clientWidth − NAMES_W`.
- 189–196: ⌘-wheel reads the rect left (192) and `scrollLeft` (194) into `anchor`.
- 204–208: a layout effect on `[ppb]` writes `el.scrollLeft`.
- 209–213: `zoom(by)` reads `scrollLeft` and `clientWidth`.

**Client → step:** 254–257, `stepOf` = `(clientX − row.rect.left)/ppb`. 292–295: velocity from the strip rect.

**Sticky**
- 359: the ruler row is `sticky top-0 z-30`.
- 360: the corner is `sticky left-0 z-40`.
- 383: each row-name button is `sticky left-0 z-20`.

**Shared:** it uses NoteView's exported `Ruler` (`NoteView.tsx:1842`) and `PlayLine` (`NoteView.tsx:1902`).

---

## 2. What each view renders (culling)

**Arrangement: everything is rendered.**
- All tracks render (`SongTimeline.tsx:2344–2360`).
- Every lane row of every track renders (2634–2645).
- Every clip renders (`LaneClips` 2983–3032, each `Clip` absolute at `left = start*ppb` 3130–3133).
- Every note of every notes clip is an SVG rect (3180–3190).
- Bar and beat lines are SVG `VLines`, one per meter span across the full `width` (2035–2054). The ruler's bar numbers and lines span the full width (2195–2231).
- The only lazy part is the audio waveform. `Wave.tsx:268–269` cuts each clip into 1024 px canvas tiles. Each `Tile` draws only after an IntersectionObserver with `rootMargin: "0px 256px"` sees it (`Wave.tsx:161–170`). The `seen` flag never resets.
- The `visible` state is used only for content width (427), follow `reach` (422) and centring the empty state (2383). It does not cull.
- `look({ seen })` (586) is reported to agents only. Nothing renders from it.

**Graph: everything is rendered.**
- Every node renders (`ordered.map`, 880–896). Every wire is an SVG path, plus a hit path per wire (842–878). Every WirePlus renders (897–914).
- Visibility only gates network "looking": `lib/taps.ts:59–75` (`useLooking`, IntersectionObserver plus `visibilitychange`) is used by `PictureView.tsx:206` and `CardViews.tsx:283` (scope). This is TAP-16 / DV-10.

**Piano roll: everything is rendered.**
- All 128 key rows, or the folded rows (`Keys` 1726–1765).
- Every note as a div (1288–1339).
- Every velocity bar (1416–1446).
- `Lanes` HBands and VLines over the full width and height (1819–1834).
- `visible` is only a minimum width (1093) and centres the empty state (1378).

**Beat grid: the only view with culling.**
- 146–147: `left` state, updated from `onScroll` (355).
- 175–177: `from = left/ppb − 4`, `to = (left+visible)/ppb + 4`, `near()`, and `shown = steps.filter(near)`.
- `Cells` renders only `shown` steps (527–548) and hits filtered by `near` (402, 549–562). The velocity strip renders only `near` hits (446).
- Bar lines are all rendered (365–367).
- The KIT-10 "Today" note documents the culling (`architecture.md:2104`). `beat-grid.mjs:248–250` tests it.

---

## 3. Zoom

### Arrangement

**State**
- `lib/zoom.ts:7`: `ZOOM = {min 2, max 192, start 24, step 1.5}`.
- `lib/zoom.ts:15–16`: `useZoom()` and `zoomNow()` read `useLocal("zoom")`, which is persisted per song in `.music2/view.json` (FMT-20).
- `lib/zoom.ts:19, 22–26, 29`: `roundZoom`, `setZoom`, `stepped`.
- Consumers: `SongTimeline.tsx:340, 420, 2037, 2091, 2468, 3104, 3170, 3277, 3298`; `Wave.tsx:232`; `TakeRows.tsx:100`; `Take.tsx:23`; `DropLayer.tsx:22`; `drop.ts:87`.
- Snap follows the zoom: `grid.ts:87–90` (`snapSong(.., ppb)`), `grid.ts:142–147` (`gridOp`), and `SongTimeline.tsx:1681`.

**Width, `spare` and `room()`**
- `SongTimeline.tsx:317–322`: `room(meter, end, beat)` is one bar after the bar the last clip or playhead is in.
- 424: `spare` state.
- 427: `width = max(room(...)*ppb + TOOLS_W, visible, spare)`.
- 479: `setSpare(max(0,left)+view)` keeps the scroll target reachable after a zoom.
- 1963: `TOOLS_W`.

**`flushSync`**
- 477–480: `flushSync(() => { setZoom(z); setSpare(...) })` runs before `scrollTo(left)` (481), so the new width exists before scrolling.
- 1253: also `flushSync` for select on clip press. Not zoom.

**Keys and Fit**
- ⌘= and ⌘−: `App.tsx:121–123`, then 453–458. The roll gets it first via `editorHooks.zoom`, otherwise `canvas.zoom` = `zoomStep` (492–498). It zooms around the playhead when in view, else the middle (Q9).
- Z / Fit: `App.tsx:76` maps `z` to `zoom_fit`, routed at 460–461 to `canvas.zoomFit`.
- `zoomFit` (500–518) fits the selection range, the picked clips or the whole song with `FIT_PAD` 16 (1961). `fitted` (471) allows Z-back to the saved zoom and scroll (503–507). `zoomTo` clears `fitted` (488).
- Buttons: `ViewTools` (1969–2017) has zoom-out (1990–2002) and zoom-in (2003–2014), disabled at the min and max.

**Ctrl-wheel and pinch**
- 596–601: Ctrl/⌘ wheel, with `deltaMode 1` → ×33, clamped ±100, `ZOOM.step ** (−dy/100)`, around the pointer.
- 602–608: `gesturestart` saves `z0`; `gesturechange` calls `zoomTo(z0*scale, at(clientX))`; `gestureend` only `preventDefault`s.
- 609–614: listeners are non-passive on the scroller.
- `canvas` registration 576–581 (`lib/shell.ts:214–228`).

### Graph

**State**
- `NodeGraph.tsx:182–194`: `zooms: Record<module, number>` in component state, not persisted. It is lost on unmount; `Dock.tsx:153` mounts NodeGraph only while view = graph.
- 184: `zoom = zooms[module] ?? fit`.
- 254: `clampZoom` 0.3–2.

**Fit**
- 260: `FIT = 1`.
- 281–283: `fitZoom()` ignores its arguments and returns `FIT`, so `room0` is measured but has no effect on the result.
- 156: `collapsed` layout for fit (DV-13).

**ZoomBar** (286–324, rendered at 246 outside the scroller, `absolute right-8 bottom-8`)
- ÷1.2 (292), 100% (295), ×1.2 (298), Fit = `onZoom(null)` (302–311), Collapse all (312–321).
- `data-zoom` (291) is read by tests.

**Ctrl-wheel and pinch**
- 202–206: `zoom * exp(−deltaY/240)`. It is not anchored at the pointer; there is no scroll compensation.
- 207–215: `gesturestart` and `gesturechange`. No `gestureend` handler, and no anchoring.
- Applied by CSS `scale(zoom)` with `origin-top-left` (837–838) inside a box sized `width*zoom` (834).
- No `flushSync`. No keyboard zoom (`editorHooks.zoom` is NoteView only).

### Piano roll

**State**
- `NoteView.tsx:165`: `ppb` is `useState(48)`, not persisted.
- 255: on open it is fitted to the clip, `(clientWidth − keysW − 32)/length`, clamped 12–160.
- Zoom range 8–400 (278, 295).

**Zoom inputs**
- ⌘-wheel at 272–279 (`exp(−deltaY*0.004)`, pointer-anchored through `anchor` plus the layout effect at 287–291).
- Buttons ×0.8 and ×1.25 (1225–1230).
- ⌘= and ⌘− through `editorHooks.zoom` (`zoomHook`, 731, ×1.5 when the roll is focused; `App.tsx:457`).
- No pinch: there are no gesture listeners.

**Vertical zoom:** rows 10–24 px (`ROW_MIN`/`ROW_MAX`, 53–56), persisted in `roll.rows` (149–151). `resizeRows` (300–306) runs from ⌘↑/⌘↓ via `editorHooks.rows` (732; `App.tsx:476`). The middle row is kept by 298–311.

**`flushSync`:** none. A layout effect does the anchoring instead.

### Beat grid
- `BeatView.tsx:144`: `ppb` is `useState(88)`, not persisted. Range 16–400 (195, 212).
- ⌘-wheel at 189–196 (pointer anchor plus the layout effect at 204–208). Buttons ×0.8 and ×1.25 (339–344).
- No keyboard zoom, no pinch, no `flushSync`.
- 171: content width grows bar by bar until `end*ppb ≥ visible`.

---

## 4. Follow-playhead and other programmatic scrolling

**Follow (arrangement only)**
- `lib/zoom.ts:32–34`: `useFollow`, `followNow`, `setFollow`, persisted in view.json.
- `SongTimeline.tsx:421–422`: while playing and following, content reaches `beat + visible/ppb`, so a page always has room.
- 443–458: an `onFrame` loop pages when the playhead is past 85% or outside the view, putting it at 15%. It respects `paused`.
- 459–467: a scroll by hand while playing pauses follow. It resumes on the next play (`paused = false` at 445) or when the playhead is back in view (452–455).
- Toggles: the button at 1977–1989; ⇧⌘F at `App.tsx:125` and 462–463 (`canvas.follow`, 577).
- The roll and beat grid never follow. Their `PlayLine` and `PlayCap` (`NoteView.tsx:1902–1935`) only move by `translateX` and hide past the width.

**Playhead drawing (all views)**
- `SongTimeline.tsx:339–351`: `usePlayhead` sets `translateX(beat*ppb)` every frame.
- Used by `PlayLine` (353–357, rendered at 1901–1905 and 1953) and `PlayCap` (2020–2028).

**Markers**
- `SongTimeline.tsx:532–554`: Add Marker scrolls the bar into view if `x<0 || x>view−200` (547–552), then opens the field.
- Previous and next marker (538–541) only `onSeek`; they do not scroll. Keys are ⌥←/⌥→ at `App.tsx:67`, routed at 482.

**Agent Show (AGENT-6)**
- `lib/pointer.ts:135–162` covers each target kind:
  - Bars: `canvas.reveal` = `showBeats` (`SongTimeline.tsx:569–575`).
  - Lane or clip: unfolds the track, then `scrollIntoView` (inline center for a clip).
  - Track: header `scrollIntoView`.
  - Knob: `expandFor`, possibly switching the dock to Graph, then `scrollIntoView`.
- `PointerNote.tsx:140` holds the Show button.

**Reveal and lit lanes (TL-16)**
- `SongTimeline.tsx:1735–1771`: `reveal()` runs on the `LANE_MADE` and `TRACK_MADE` window events (`lib/events.ts:63–68`) and after timeline, open or hidden changes (1752).
- It calls `row.scrollIntoView({ block: "nearest" })` (1746), which needs the scroll-padding at 1822.

**Graph DV-13 and DV-18**
- `NodeGraph.tsx:1237–1246`: the node's `scrollIntoView` 290 ms after expanding, after the 280 ms move (1253, `lib/expanded.ts:39–43`).
- `PictureView.tsx:323–333`: the column's `scrollIntoView` 320 ms after a dot is picked.

**Roll and beat grid**
- `NoteView.tsx:262`: initial `scrollTop` centred on the clip's pitches.
- `NoteView.tsx:290` and 310: zoom and row anchors.
- `BeatView.tsx:207`: the zoom anchor.

**Native focus that scrolls (not preventScroll)**
- `TrackName.tsx:80`: `autoFocus` rename field in the sticky header.
- `NodeGraph.tsx:2303`: `autoFocus` type-in on a bar inside the graph.
- `Knob.tsx:365–369`: `focusNext` calls `.focus()` on the next slider (Tab in a type-in). It is used by `NodeGraph.tsx:2321`.
- Tab between node headers and bars (`NodeGraph.tsx:1426`, `Bar.tsx:66`, DV-15) relies on the browser scrolling focus into view.
- The marker field (`Markers.tsx:206–218`) and RepeatPopover (`SongTimeline.tsx:2886`) autofocus inside portals, so they do not scroll the timeline.

---

## 5. Contract rules mentioning scroll, zoom, follow, Fit, sticky, reveal or "in view"

Note: Q8, Q9, Q11, Q31, X25 and X33 are **not** in taste.md or architecture.md. They are defined in `polish-plan.md`.

### taste.md
- §2.12, 444–445: zoom icons `ZoomIn`/`ZoomOut`; follow icon `ArrowRightFromLine`.
- §2.13, 463: no motion for "the playhead, meters, knob, fader and cable drags, zoom, scroll".
- 466–467: under reduced motion the new lane is "still selected and scrolled into view".
- §3.1, 696–698: "The Song row is pinned to the bottom of the track list" (sticky bottom).
- §3.2, 710: ruler row. The timeline is the song's room plus the tools' width, "so the last bar numbers scroll clear of Follow and zoom".
- §3.2, 714: arrangement zoom 24 px a beat; 2–192; ⌘=/⌘− ×1.5; pinch; ruler numbers thin out.
- §3.5, 818–820: Graph "Fit is 100%, top-left aligned … A graph bigger than the panel scrolls, across first … Expanding a node never changes the zoom."
- 824: Fit's 100% is `FIT`, tested by `nodes`.
- 829–830: the zoom bar (zoom out, the zoom, zoom in, Fit, Collapse all) floats at the panel's bottom right.
- 839: Notes header tools include "vertical zoom".
- §3.8, 979–980: "Nothing scrolls horizontally except the arrangement, the roll, the Graph and the Mix strips."
- **TL-9**, 1037: playhead moved with `requestAnimationFrame` from the transport clock, no transition.
- **TL-11**, 1039: zoom ⌘=/⌘−, pinch, Z (selection or whole song); heights with ⌘↑/⌘↓.
- **TL-16**, 1044: "Revealing a lane doesn't hide the first name (D12)": set `scroll-padding-top` to the ruler's height and reveal with `block: "nearest"`.
- **DV-5**, 1096: Fit 100%, top-left; "a graph bigger than the dock scrolls"; layout is a pure function.
- **DV-10**, 1116–1118: the scope asks only while on screen and stops when "scrolled out of sight". Today it uses an IntersectionObserver.
- **DV-13**, 1128: expand or collapse keeps the zoom ("Fit isn't worked out again") and "the graph scrolls the node's top into view".
- **DV-14**, 1132: "The wheel never edits a bar: it scrolls the graph, wherever the pointer is (a picture's dot keeps its wheel, DV-17)."
- **DV-15**, 1136: Tab goes through the nodes (focus-driven scrolling); PageUp, PageDown, Home and End step a focused bar.
- **DV-16**, 1140: Show "expands the node and scrolls to that value" (AGENT-6).
- **DV-18**, 1148–1150: a picked dot lights its column "and scroll[s] the graph to it".
- **NT-5**, 1164: vertical zoom with ⌘↑/⌘↓, rows 10–24; Fold.
- §5.1, 1259: Follow the playhead is ⇧⌘F, on by default, kept per song.
- §5.1, 1282–1283: zoom in/out ⌘=/⌘− and pinch; Z zooms to the selection, again goes back.
- 1332: on faders and knobs, "scroll or two-finger swipe: one step per notch" (wheel captured inside scrollers).
- 1367–1368: the Transport menu lists Follow; the View menu lists Zoom In, Zoom Out and Zoom to Selection.
- B24, 1513: Follow is on by default.

### architecture.md
- **FMT-20**, 854/859: `.music2/view.json` holds `zoom` (2–192), `follow`, `heights`, `hidden`, `grid`, and `roll` (rows).
- **AGENT-6**, 1203: the focus record includes "the bars the timeline shows". A pointer "MUST NOT move the musician's view … Off screen, it waits at the edge with Show: only that click moves the view".
- **AGENT-6 Today**, 1205: Show on a +N knob "expands the node and scrolls to it". Tested by `e2e/ag-focus.mjs`.
- **NOTE-8**, 1543–1544: folded rows keep "the whole kit in view". Why: "scrolling past the 25 empty ones hides the beat".
- **NOTE-9**, 1547: row height 10–24; ⌘↓/⌘↑ while the roll has the keyboard.
- **KIT-10**, 2102: the beat grid "scrolls and zooms (⌘ + wheel, its zoom buttons) as Notes does".
- **KIT-10 Today**, 2104: "It draws only the steps near what shows".
- **TAP-4**, 2763: a tap costs nothing while no one looks.
- **TAP-8**, 2779: scope asks.
- **TAP-16**, 2811–2813: ask only "while what shows it is on screen". Today: `useLooking` in `lib/taps.ts`, an IntersectionObserver.

### polish-plan.md
- Q8, 46: zoom range and steps.
- Q9, 47: zoom around the playhead or middle (keys), the pointer (pinch, ⌘-scroll), Z and back.
- Q11, 49: follow pages at 85%; a scroll by hand pauses it.
- Q31, 69: Follow and zoom buttons at the ruler's right end.
- X25, 1061/1148: the buttons are `sticky right-0` in the ruler row.
- X33, 1161: zoom is any value; tests write `zoom` into view.json.

---

## 6. Browser suites that depend on the scrollers

**Implicit dependency in almost every suite.** Puppeteer's `page.click(sel)`, `ElementHandle.click()` and `hover()` scroll the element into view natively (CDP `DOM.scrollIntoViewIfNeeded`) before acting. `page.focus` behaves the same way, for example `pl-notes.mjs:353`. So every click on a clip, header, node, bar or note inside these scrollers depends on native scrolling. For example `fixlib.mjs:109`, `show(module)`, clicks `[data-module-tab]` in the timeline's sticky header column. These are not counted below.

### Arrangement (`[data-testid=timeline]`)
- **pl-zoom.mjs (4 explicit; follow and Z depend on them throughout):**
  - 26–27: `scroll()` and `setScroll()` read and write `scrollLeft`.
  - 29 and 60–70: the playhead's screen x stays put across ⌘=/⌘−.
  - 85–101: ⌘-wheel at 91 and Ctrl-wheel at 97; the beat under the pointer stays; 102 checks the page never zooms.
  - 106–119: Z fills the view; Z again restores `ppb` and `scrollLeft` (119).
  - 120–126: Z with nothing selected fits the whole song.
  - 160–197: follow. Scroll grows past 2000 and 5000 (167, 171); the playhead is near 15% (168–169); `setScroll(0)` stands in for a scroll by hand (174–176); play resumes following (180); turning it off holds the view (183–186); ⇧⌘F resumes (189).
- **layout.mjs (3 timeline + 2 Graph):**
  - 103 and 149: `scrollLeft = 0`.
  - 104–107: a click at the ruler label's rect seeks.
  - 112–119: the playhead moves on screen.
  - 150: drawn width / 24.
  - 160–170: the header stays sticky (`header.left − box.left ≈ 0`); `scrollWidth − (header+ruler) ≤ 1`; the playhead stays inside the ruler past the drawn end.
  - 133 and 208: Graph knob `scrollIntoView`.
- **nx-nits.mjs (3 timeline + 1 roll; the 4 Mix-strip hits at 151–160 are out of scope):**
  - 87: `scrollWidth > clientWidth`.
  - 89: `scrollLeft = scrollWidth`, then 91–93 check that bar numbers end left of the sticky `view-tools`.
  - 94: reset.
  - 127: roll key row `scrollIntoView`.
- **ag-focus.mjs (5):**
  - 56–64 (`viewNow`): reads timeline `scrollLeft`/`scrollTop` (60–61) and roll `scrollLeft`/`scrollTop` (58, 62).
  - 130–141: Ctrl-wheel (136) until the focus record's `seen[1] < 20`.
  - 143–148: out of view the note waits at the right edge and the view is unchanged.
  - 150–152: Show scrolls the timeline (`after.left > seen.left`).
- **live3a.mjs (1 timeline + 2 roll):** 59 `scrollLeft = 0`; 52–57 bar lines from the ruler rect; 90–94 comment that the view is still scrolled from following; 135 and 197 roll `scrollTop`.
- **st-audio.mjs (3):** 196 and 232 clip `scrollIntoView`; 406 `timeline.scrollTo({left:0})`; 195 "waveforms draw" depends on the Wave IntersectionObserver.
- **st-midi.mjs (1):** 214 `timeline.scrollTo({left:0})`, then the ruler box.
- **own-gate.mjs (2 timeline + 1 Graph):** 233 and 288 clip `scrollIntoView({inline:"start"})`, then edge-drag maths at PPB 24; 40 is a Graph grip.
- **own-base.mjs (1 + 1):** 177 the same clip `scrollIntoView`; 34 is a Graph grip.
- **comp.mjs (1):** 88 uses `.sticky` as a CSS selector for take-row names (`TakeRows.tsx:160`).
- **sx-follow.mjs (0 explicit):** it is about Follow *tempo*, not the playhead. 54–76 read waveform canvas pixels, which exist only after `Wave.tsx:167`'s IntersectionObserver sees the tile.
- **kits.mjs:19, own-gate.mjs:153, pc-keys.mjs:23:** rely on `[data-song-row]` being a separate subtree (structure only).

### Piano roll (`[data-testid=roll-scroller]`)
- **notes.mjs (6):**
  - 72: 128 key rows rendered.
  - 73–77: notes counted in view via the scroller rect (74); "opens centred".
  - 82, 179, 343, 389: `scrollTop`/`scrollLeft` writes.
  - 221–223: reads `scrollTop`/`clientHeight` to pick a pitch.
- **fixes-notes.mjs (4):** 65 and 124 `scrollTop`/`scrollLeft` writes; 94 and 134 note `scrollIntoView`.
- **nt-notes.mjs (2):** 57 key row `scrollIntoView` (helper `cell`); 107 note `scrollIntoView`; 339 and 344 ⌘↓/⌘↑ row height.
- **pl-notes.mjs (2):** 52 note `scrollIntoView`; 60 scroller rect for empty grid; 352–367 ⌘=/⌘− zoom the roll with `page.focus` at 353.
- **g11-play.mjs (1):** 71 `scrollTop`.
- **s4-clips.mjs (1):** 23 the scroller box gives the click y.
- **st-guide.mjs (1):** 117 the same.
- **s8-audition.mjs (1):** 53 "wholly in view" notes via the scroller rect.
- **nx-nits.mjs:127** and **ag-focus.mjs:58, 62:** counted above.

### Beat grid (`[data-testid=beat-scroller]`)
- **beat-grid.mjs (2):** 248 checks far steps are not drawn (culling); 249 `scrollLeft = scrollWidth`; 250 the last bars' steps are drawn after the scroll; 251 reset.

### Graph (`[data-testid=graph]`)
- **pictures.mjs (7):**
  - 190: DV-18 lit column.
  - 192: column knob `scrollIntoView`.
  - 216–226: wheel on a dot (221, 224) must leave graph `scrollTop + scrollLeft` unchanged (217, 226).
  - 294 and 320: picture `scrollIntoView`.
  - 308–318: Ctrl-wheel zoom over the bare graph (317) changes the picture's on-screen size.
  - 330: the Fit button.
- **card-views.mjs (4):** 46 and 137 `scrollIntoView`; 183–184 pointer Show; 304 `graph.scrollTo({0,0})`; 307 `graph.scrollTop +=` to scroll the scope out of sight, after which the IntersectionObserver must stop the ask.
- **ports.mjs (5):**
  - 30 and 37: `scrollIntoView` helpers.
  - 186–191: `graph.scrollLeft/scrollTop +=` to centre two ends.
  - 94–99: zoom from the box/svg width.
  - 232–270: zoom bar and Fit; 237–238 compare graph `scrollWidth/Height` with `client*`.
  - 272–358: Fit with real scrollbars; a second browser without `--hide-scrollbars` and a `barRoom` probe mirroring `scrollbarRoom`.
- **nodes.mjs (1):**
  - 132–133: Fit is `data-zoom` 1.00.
  - 188–194: the wheel over a bar must scroll the graph and write nothing (192).
  - 231–269: expanding keeps the zoom; `portAt` at 233–238 is "read in one frame (the graph may be scrolling)".
  - 502–503: pointer Show.
  - 152, 263 and 533 are text-overflow checks, not scroller checks.
- **cables.mjs (1):** 63 `scrollIntoView` in the middle of a cable drag ("as a hand would").
- **fixes-graph.mjs (1):** 53 `scrollIntoView`; 75–80 the zoom bar box sits inside the section.
- **Long tail of "scroll the knob or card into view first" helpers** (one to three each): ab-sampler 108; attention 221; br-plugins 86; cards 98, 150; dress 68 (plus a dot wheel at 217); dress2a 40; dress2a-strip 31; dressed 29; dsp 34, 142; flanger-grains 51; graph 41; instruments 65; live3b 83; mixing 30; own-edits 28, 209, 275; own-window 22; pl-lanes 118, 132; s5-knobs 144, 152, 202; s6-builders 128, 175, 191; s8-automation 38; start 49; taps2 110; tune 57; vocoder 81; voice 79; wavetable 58, 94, 109. Some of these (start, live3b, gestures 28) also target Mix strips.

**Out of scope but showing up in the grep:** s8-mix (8, Mix strips), nx-nits 151–160 (Mix), gestures 185 (Mix pan wheel), s6-fixes 116 (attention panel), and fx-fixes, s8-songbar, pc-keys and sound-stops (text overflow).

---

## 7. Anything else that breaks if the scroller stops being a native scroll box

### Keyboard scrolling
- **Timeline.** The scroller itself is focusable (`tabIndex -1`, 1817) and is focused on clip, point and menu presses (996, 1075, 1254, 1540).
  - `onKeyDown` (1776–1801) handles only ⌫/Delete, Q, ←/→ with clips picked, and Esc. Everything else falls through to native scrolling: ↑/↓, ←/→ with no selection, PageUp/PageDown and End.
  - `App.tsx:73` maps Home to "home" but does not `preventDefault` it (204 only covers play, `view_*`, shortcuts and `*_marker`). So Home probably still scrolls natively too.
  - Space is `preventDefault`ed (`App.tsx:70, 204`).
- **Roll.** Focus sits on `root` (tabIndex 0, `NoteView.tsx:1128`), an ancestor of the scroller. Arrows are handled only with notes picked (790). Native arrow scrolling probably doesn't reach the inner scroller; I haven't verified this.
- **Graph.** Focusable headers (`NodeGraph.tsx:1426`) and bars (`Bar.tsx:66`) sit inside the scroller. Unhandled arrow and page keys, plus Tab focus, scroll it natively (DV-15).
- **Beat grid.** No focus target of its own. Native keyboard scroll after a click depends on browser heuristics.

### Wheel
- Plain wheel and trackpad scroll are entirely native in all four views. The only custom handlers are ⌘/Ctrl zoom (see §3).
- Children that `preventDefault` to stop scrolling: `Knob.tsx:495–513` and `PictureView.tsx:533–550`. `Bar.tsx:60` deliberately lets the wheel through (DV-14).

### Drag auto-scroll at the edges
- **None implemented** in any of the four views. There is no `scrollBy` anywhere. This covers clip drag (1254–1380), marquee (849–901), note drag (`NoteView.tsx:870–923`), cable drag (`NodeGraph.tsx:744–796`), header reorder (`Tracks.tsx:347–371`) and ruler loop drag.
- HTML5 drag-and-drop gets the browser's own edge autoscroll for free:
  - timeline drop targets `SongTimeline.tsx:2364–2375` (instrument onto the empty space) and 2617–2631 (effect or sound onto a track);
  - the Dock at `Dock.tsx:120–125`;
  - file drops through window listeners at `lib/drop.ts:207` and 284, resolved by `target()` (77–101) and outlined by `DropLayer.tsx`.
- Several drags use rects captured at press or deltas, so they are not compensated if the view moves mid-drag: `SongTimeline.tsx:1000, 1084, 1218, 1279, 2154, 2287`; `TakeRows.tsx:108`; `Wave.tsx:280`.

### Popovers and menus anchored inside the scrollers
- `ui.tsx:50–124` (Popover) is placed **once** in a layout effect from `anchor.getBoundingClientRect()` (70–81, "it never moves after"). It is `fixed` and portaled. It closes on resize (99–102), not on scroll.
- Popovers anchored to elements inside the timeline:
  - MarkPopover (`SongTimeline.tsx:2233–2244`) on a ruler chip;
  - RulerMenu on a 1 px span (2245–2260);
  - the marker field (`Markers.tsx:206`);
  - track menus via `[data-track-menu]` / `[data-module-tab]` (`Tracks.tsx:109`).
- Menus at a fixed client point: `PointMenu` (`ClipMenu.tsx:63`), RepeatPopover (`SongTimeline.tsx:2873`), VolumePopover (`Wave.tsx:527`), NoteMenu (`NoteView.tsx:1385`), DropMenu (`NodeGraph.tsx:1093`).
- Graph popovers: effect-menu (1777), listens-menu (2136), PresetPopover (1615) and KnobMenu (2337, 2492).
- `EffectBrowser.tsx:146–160` (opened from WirePlus, `NodeGraph.tsx:1058`) follows its anchor with `window.addEventListener("scroll", follow, true)` and resize; `place()` is at 93–101.
- `Knob.tsx:643–653` (KnobMenu) closes on `document.addEventListener("scroll", onClose, true)`.
- `PointerNote.tsx:92–108` re-places itself every animation frame.

### Canvases and SVG sized to the content
- No canvas is sized to the scroll area. Wave tiles are at most 1024 px (`Wave.tsx:27, 178`); meters use their own box (`lib/meters.ts:288–293`); picture and scope canvases use their own box.
- SVG pattern layers are content-sized and grow with zoom:
  - timeline `Grid` VLines per meter span (2041–2051);
  - ruler lines (2196–2199);
  - roll `Lanes` HBands and VLines (`NoteView.tsx:1821–1832`) across `width × 128*rowH`;
  - Graph `Dots` (`NodeGraph.tsx:833`) filling the canvas min size;
  - `draw.tsx:14–75`.

### ResizeObserver uses
- Visible width: `SongTimeline.tsx:410–415`, `NoteView.tsx:267–270`, `BeatView.tsx:184–187`.
- Graph room: `NodeGraph.tsx:166–181`.
- Timeline corner: `KeyboardPiano.tsx:31`.
- Clip height: `Wave.tsx:253–260`.
- Picture size: `PictureView.tsx:185`.
- Meters: `lib/meters.ts:289`.

### IntersectionObserver uses
These rely on viewport clipping by the scroll ancestors.
- `Wave.tsx:167`: lazy tile drawing. Its `rootMargin` applies to the viewport only.
- `lib/taps.ts:65`: scope and picture looking, DV-10 / TAP-16.

### Other couplings
- **Velocity strips mirror `scrollLeft`.** They sit outside the roll and beat scrollers: `NoteView.tsx:1239, 1405` and `BeatView.tsx:353–355, 437`.
- **Beat-grid culling is driven by `onScroll`** (`BeatView.tsx:355`).
- **Agent `seen` is driven by the native `scroll` event** (`SongTimeline.tsx:588`).
- **Follow's pause detection depends on scroll-event semantics** (`ours` and `across`, 430, 459–467).
- **Scrollbar-room logic in the Graph** (`NodeGraph.tsx:169–175, 266–273`) and the ports.mjs "Fit with scrollbars" test assume native scrollbars. `index.css:124` sets `scrollbar-width: thin` globally.
- **`overflow:hidden` boxes still move programmatically.** Puppeteer clicks, `scrollIntoView` calls in app code (`pointer.ts:138`, `SongTimeline.tsx:1746`, `NodeGraph.tsx:1241`, `PictureView.tsx:332`) and native focus all scroll *any* overflow ancestor, including `overflow:hidden` ones. If the replacement viewport is still an overflow box, those calls would shift its native scroll offset underneath a transform-based camera.

---

## Component sizes and render structure

`SongTimeline.tsx` is 3,314 lines, `NodeGraph.tsx` 2,508, `NoteView.tsx` 1,935 and `BeatView.tsx` 611.

**SongTimeline** is one focusable scroller around a content div of width `headerW + width`:
- A sticky top ruler row: a sticky-left corner (+Track, GridChip, fold-all); the DOM `Ruler` with section band, loop brace, SVG bar lines, DOM bar numbers, markers and chips; sticky-right `ViewTools`; `PlayCap`.
- Absolute loop and section overlays.
- `Tracks` (memo), then `TrackRows` per track. Each row is a flex of a sticky-left header (`TrackHeader` with a canvas meter, or `LaneLabel`) and a full-width lane div. The lane holds an SVG `Grid` and `LaneClips`. Each clip is an absolutely positioned div holding a `Notes` SVG, a `Curve` (SVG polyline plus DOM dots), `Steps` (DOM) or `Wave` (canvas tiles, drawn when seen).
- `TakeRows` and `HiddenRow`, a filler row with the empty state, the absolute `PlayLine`, drag guides, marquee and bubbles, portaled menus.
- A sticky-bottom Song row holding its own `TrackRows` and `PlayLine`.

**NodeGraph** is a scroller around `ModuleSection`: a `data-canvas` (an SVG `Dots` background), then a box `width*zoom × height*zoom`, then `area` with `transform: scale(zoom)`. Inside `area` are an SVG wire layer (paths plus hit paths), absolutely positioned DOM `Node` cards (header, rows, `Bar` sliders, picture SVGs and live canvases, scope canvases), DOM `WirePlus` buttons and an SVG cable overlay. The ZoomBar floats outside the scroller.

**NoteView** is a focusable root holding:
- An inner scroller around a content box `keysW+width × ROLL_RULER_H+128*rowH`. Inside it: a sticky top row (sticky-left corner and the DOM `Ruler`), then a flex of a sticky-left `Keys` column (128 or folded DOM rows) and the grid div. The grid holds SVG `Lanes`, a hatch past the clip's end, every note as a DOM div, ghosts and stamp label, the marquee and `PlayLine`.
- Below the scroller, a velocity strip whose inner div is `translateX`-mirrored.

**BeatView** is a scroller around a content box `NAMES_W+width × ROLL_RULER_H+rows*44`:
- A sticky top row with the shared `Ruler`.
- Absolute bar lines.
- Per kit row, a flex of a sticky-left name button and `Cells`, which holds absolute DOM step divs (culled to near the view) and off-grid hit marks.
- `PlayLine`.
- Below the scroller, a mirrored velocity strip (culled).

Everything in all four views is DOM or SVG except the waveform tiles, meters and picture/scope canvases.
