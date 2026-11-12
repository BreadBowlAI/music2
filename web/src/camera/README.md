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
| `math.ts` | The pure functions: `move`, `to`, `zoomAround`, `reveal`, `fit`, `toContent`, `toBox`, `cull`. Each takes a camera and the view's shape, and gives a new camera or a conversion. |
| `store.ts` | The cameras: one small zustand store each, by name. `useZoomOf`, `useCull`, `onCamera`. |
| `recognizer.ts` | What wheel and pinch events mean: swipes, rails, the `cancelable` rule, zoom, and its own glide where the platform gives none. |
| `writer.ts` | Writes the camera into the box in whole pixels, and tells a move the browser made itself. |
| `spans.ts` | `useSpan`: places a box along the stretched way by the zoom, straight into its style, so a zoom renders it not. |
| `rig.ts` | One view's camera at work on its box: the recognizer's events once a frame, the writer, the app's moves. |
| `useCamera.ts` | `useCamera`, `useNear` and `controlsOf`: how a view uses all this. |
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
import { useCamera, useCull, useZoomOf, within } from "../camera";

function Timeline() {
  const box = useRef<HTMLDivElement>(null);
  // At the top of the view: its box, what covers the box's edges, where its zoomed part starts.
  const camera = useCamera("timeline", box, {
    margins: { left: HEADERS, top: RULER },
    origin: { x: HEADERS },
    zoom: { min: 2, max: 192 },
    start: () => 24,
  });
  const ppb = useZoomOf("timeline");        // renders on a zoom, never on a move
  const shown = useCull("timeline", "x").x; // the beats to draw: changes only at a step
  return (
    <div ref={box} className="overflow-auto">
      <div style={{ width: HEADERS + beats * ppb }}>
        {clips.filter((c) => within(shown, c.start, c.start + c.length)).map(…)}
      </div>
    </div>
  );
}
```

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
  never holds up a frame. For rows of their own heights, `useNear(name, ref)` says whether a row is
  within a view of the box.
- **A zoom renders nothing it can place:** `useSpan(name, ref, from, to)` writes a box's `left` and
  `width` from the zoom. Memoise what a new window keeps, so only what enters it renders.
- **Hover:** a hover handler that renders does nothing while `movingNow()`.
- **A control that keeps its wheel** (a picture's dot): mark it `data-wheel`.
- **Who moved it:** `onCamera(name, (now, was) => …)` with `now.by`: `"hand"`, `"app"`, `"browser"`
  or `"zoom"`. Follow pauses on `"hand"` and `"browser"`.
- The swipe probe calls `pauseSwipes()` while it listens: the web view then scrolls plain swipes,
  and the camera takes them in.
