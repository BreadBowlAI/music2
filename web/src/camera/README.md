# The camera

In short: a small library that moves a view as the hand does. A view keeps its own scroll box, and
the camera tells the box where to look. Every swipe over the box is the camera's: it moves 1:1 with
the fingers, both ways at once, a nearly straight swipe on its line. ⌘-scroll and a pinch zoom
around the pointer. The view draws only what is in view. Its rules are taste.md §3.10 (VIEW-1 to
VIEW-12), and why it is built this way is the camera note (`docs/designs/camera/design.md`).

## What is in it

| File | What it holds |
|---|---|
| `index.ts` | The only file the app imports. |
| `numbers.ts` | Its numbers: G (the gap that ends a swipe), the rails, the glide, the culling's margin and step. One line each, so the owner's probe report changes one line. |
| `math.ts` | The pure functions: `move`, `to`, `zoomAround`, `reveal`, `fit`, `toContent`, `toBox`, `cull`, `cullPlan`. Each takes a camera and the view's shape, and gives a new camera or a conversion. |
| `store.ts` | The cameras: one small zustand store each, owned by the view that made it, lent to what it draws (`CameraScope`) and listed by name while mounted. `useZoomOf`, `useCull`, `onCamera`. |
| `recognizer.ts` | What wheel and pinch events mean: swipes, rails, the `cancelable` rule, zoom, and its own glide where the platform gives none. |
| `writer.ts` | Writes the camera into the box in whole pixels, and tells a move the browser made itself. |
| `spans.ts` | `useSpan`: places a box along the stretched way by the zoom, straight into its style, so a zoom renders it not; `spanBox`, where such a box lies, its ends on the screen's own pixels. |
| `rig.ts` | One view's camera at work on its box: the recognizer's events once a frame, the writer, the app's moves. |
| `useCamera.ts` | `useCamera`, `useNear`, `controlsOf` and `controlsAt`: how a view uses all this. |
| `state.ts` | What every camera shares: the platform, the probe's pause, a drag holding the pointer, `movingNow`. |
| `face.ts` | What the `camera` suite sees of it, on `window.m2canvas.camera`. |

It imports nothing from the app: only its own files, React and zustand. It never reads `navigator`:
the app says where it runs with `setPlatform`. `npm run typecheck` runs `scripts/camera-bounds.mjs`,
which fails the web build on any import that crosses these bounds.

## Words

| Word | Meaning |
|---|---|
| camera | where a view looks (`x`, `y`: its box's scroll position, in px, with fractions) and its zoom |
| zoom | the view's own unit: px a beat across a timeline, the scale in the Graph |
| margins | what covers the box's edges and stays there: a sticky ruler, names, keys |
| origin | where the part of the content that the zoom stretches starts, in px (the headers' width) |
| units | the content's own measure: beats where the zoom stretches it, px from the origin elsewhere |
| swipe | wheel events less than G ms apart |

## How a view uses it

```tsx
import { CameraScope, useCamera, useCull, useSpan, useZoomOf, within } from "../camera";

function Timeline() {
  const box = useRef<HTMLDivElement>(null);
  // At the top of the view: its box, what covers the box's edges, where its zoomed part starts.
  const camera = useCamera("timeline", box, {
    margins: { left: HEADERS, top: RULER },
    origin: { x: HEADERS },
    zoom: { min: 2, max: 192 },
    start: () => 24,
  });
  const ppb = useZoomOf(camera);        // renders on a zoom, never on a move
  const shown = useCull(camera, "x").x; // the beats to draw: changes only at a step
  return (
    <CameraScope camera={camera}>
      <div ref={box} className="overflow-auto">
        <div style={{ width: HEADERS + beats * ppb }}>
          {clips.filter((c) => within(shown, c.start, c.start + c.length)).map((c) => <Clip key={c.id} clip={c} />)}
        </div>
      </div>
    </CameraScope>
  );
}

function Clip({ clip }) {
  const el = useRef<HTMLDivElement>(null);
  // Under the scope, by the camera's name: the view's own camera, never another's of that name.
  useSpan("timeline", el, clip.start, clip.start + clip.length);
  return <div ref={el} className="absolute" />;
}
```

- **The view owns its camera.** `useCamera` makes it the first time the view asks for its name, at
  `start`, and keeps it as long as the view is mounted (a view whose name changes, as the Graph's
  with its module, finds an earlier name's camera as it left it). The view's own hooks take the
  camera `useCamera` gave (`useZoomOf(camera)`); what it draws finds it by name under
  `<CameraScope camera>`. While mounted it is listed under its name, for the app outside React
  (`controlsOf(name)`, `cameraNow(name)`, the suite's `now(name)`). Another view mounted under the
  same name in the same commit (another song opened) has a camera of its own, and lists it once the
  old one has gone.

- **Draw by the zoom, never by the position.** Read the zoom with `useZoomOf`. The position
  changes every frame of a swipe; a view that needs it (a strip that scrolls with the box) hears it
  with `onCamera`, which renders nothing.
- **Move the view only through the camera** (VIEW-9): `camera.to`, `camera.reveal(element)`,
  `camera.fit([from, to])`, `camera.zoomAround(zoom, at)`. Never `scrollIntoView` or `scrollLeft`.
  A move the browser makes itself (Tab, a key, a scrollbar, a click) is taken in, a frame late (on
  its `scroll`): `camera.sync()` takes it in at once, before the app decides from where the view is.
- **Draw only what is in view** (VIEW-12): `useCull` gives the window in the content's units, a view
  each side more. It is kept while it holds what shows and half a view more each side, so it changes
  every half a view of a pan, or ×1.5 out or ×2 in of a zoom; a new one comes as a transition, which
  never holds up a frame; once what shows has left the window on screen (one asked for may not have
  landed), the new one is drawn at once (`cullPlan`). For rows of their own heights,
  `useNear(name, ref)` says whether a row is within a view of the box: coming near and going far
  render as a transition, coming into view at once.
- **A zoom renders nothing it can place:** `useSpan(name, ref, from, to)` writes a box's `left` and
  `width` from the zoom, each end on the screen's own pixels (`spanBox`: what draws over a box finds
  it with the same function, so the two never disagree). Memoise what a new window keeps, so only what enters it renders. Given
  `{ fine }`, the shortest thing the box draws, it marks the box `data-fine` while that is under
  2 px: CSS can then show marks that keep thin things visible only then.
- **Hover:** a hover handler that renders does nothing while `movingNow()`.
- **A control that keeps its wheel** (a picture's dot): mark it `data-wheel`. It takes a swipe that
  starts on it, unless a camera just carried it under a still pointer.
- **Say the content's size** as you render it, where you know it: `camera.content({ w, h })`. When
  it gets smaller (Fold, shorter rows), the camera stops at the box's new edge in that render, and
  what the view culls by it is drawn before the frame. Without it, a ResizeObserver tells, a frame
  later.
- **Who moved it:** `onCamera(camera, (now, was) => …)` with `now.by`: `"hand"`, `"app"`,
  `"browser"`, `"zoom"` or `"clamp"` (the box's edge came in). Follow pauses on a move across by
  `"hand"` or `"browser"`, and waits while `camera.swiping()`; its page is `camera.to(x, y, zoom,
  { swipe: "keep" })`, which stops the camera's own glide but drops no swipe.
- The swipe probe calls `pauseSwipes()` while it listens: the web view then scrolls plain swipes,
  and the camera takes them in.
