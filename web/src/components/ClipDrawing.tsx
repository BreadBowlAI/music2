// A clip's mini drawing (taste.md VIEW-12; the zoom note, docs/designs/camera/zoom.md, §3): its notes,
// or its automation curve's line and the shade under it, on one canvas a clip, in place of hundreds of
// SVG shapes. A zoom only stretches the canvas with its clip, which costs the browser almost nothing;
// it is drawn again, sharp at the screen's own pixels, once the zoom settles.
//
// - Kept by its clip: a canvas is drawn again only when what it draws changes (its notes, its curve,
//   its colour), when its size changes outside a zoom by the hand (a zoom by the app, a lane's
//   height), and once a zoom by the hand settles: no zoom for G (`SWIPE_GAP`), nor in a whole frame
//   (one slower than G is no pause of the hand), checked at each frame's start. A pan draws none again,
//   unless what shows reaches past the part of its clip a canvas holds: that one at once.
// - Bounded: a canvas holds only the part of its clip in view and up to a view more each side (the
//   views' culling margin), and never a bitmap over MAX_W × MAX_H device px.
// - Its size is its drawing area's, from one ResizeObserver: told after layout and before paint, so a
//   canvas drawn there shows in that frame, and nothing here reads the page's layout.
// - On the CPU (`willReadFrequently`): the browser paints it into its clip's layer. WebKit gives a
//   canvas on the graphics chip a layer of its own, and hundreds of layers cost more than they save
//   (the zoom note's §3).
// - Each canvas keeps what it drew last as `drawn`, in px of its box, for the suites (e2e/drawlib.mjs).

import clsx from "clsx";
import { memo, useLayoutEffect, useRef } from "react";
import { CULL_MARGIN, FINE, SWIPE_GAP, cull, useCameraOf, type CameraState, type CameraStore } from "../camera";
import { countDraw, useRenderCount } from "../lib/frames";
import type { ControlClip, NoteClip } from "../lib/graph";
import type { LaneScale } from "../lib/laneScale";
import type { TrackColor } from "../lib/palette";
import { line } from "../lib/points";
import { ARRANGEMENT } from "../lib/zoom";

/** A clip's drawing area sits this far inside its border, at its top and its foot. */
export const BODY_TOP = 4;
export const BODY_BOTTOM = 4;
const AREA = { top: BODY_TOP, height: `calc(100% - ${BODY_TOP + BODY_BOTTOM}px)` };

/** The biggest bitmap a clip's canvas makes, in device px across and down (VIEW-12). */
export const MAX_W = 8192;
export const MAX_H = 512;

/** What a canvas drew last, in px of its own box as it was then. */
export interface Drawn {
  /** The camera's zoom it was drawn at, in px a beat; null where no camera lends it one. */
  zoom: number | null;
  /** The beats of its clip it holds, from the clip's start. */
  from: number;
  to: number;
  /** Its box then, in px, and its bitmap's device px a px. */
  w: number;
  h: number;
  scale: number;
  /** The colour it drew in. */
  ink: string;
  notes: { x: number; y: number; w: number; h: number; alpha: number }[];
  /** Each note's 2 px mark, while the clip's shortest note is under 2 px. */
  marks: { x: number; y: number; w: number; h: number; alpha: number }[];
  /** An automation curve's vertices; its shade runs under them to the foot. */
  line: [number, number][];
}

/** What a drawing draws with: in px of its drawing area, each shape also noted in its `drawn`. */
interface Pen {
  /** A note's box in the ink at `alpha`; with `mark`, a 2 px mark down its middle too. */
  note(x: number, y: number, w: number, h: number, alpha: number, mark: boolean): void;
  /** A curve through `xy`: the shade under it to `foot` at 0.1, then its 1.5 px line. */
  curve(xy: [number, number][], foot: number): void;
}

/** What a drawing draws, `k` px a beat and `h` px tall, holding beats `from` to `to` of its clip; `fine`: its notes get marks. */
type Paint = (pen: Pen, k: number, h: number, from: number, to: number, fine: boolean) => void;

interface Drawing {
  canvas: HTMLCanvasElement & { drawn?: Drawn };
  /** Its drawing area: the canvas holds a part of it. */
  area: HTMLElement;
  cam: CameraStore | null;
  /** Where its clip starts in the song, and how long it is, in beats. */
  start: number;
  length: number;
  /** The shortest thing it draws, in beats: under `FINE` px at the zoom, its notes get marks (0: always; null: never). */
  fine: number | null;
  ink: string;
  paint: Paint;
  /** Its area's size, in px, as the observer last told it. */
  size: { w: number; h: number } | null;
  /** The beats of its clip it holds now; null until drawn. */
  holds: [number, number] | null;
  /** Stretched by a zoom: drawn again once the zoom settles. */
  stale: boolean;
  ctx: CanvasRenderingContext2D | null;
}

/** Each camera's drawings, and what it does with them. */
interface Group {
  drawings: Set<Drawing>;
  off: () => void;
  /** A zoom by the hand is on: drawings stretch until it settles. */
  hand: boolean;
  /** When its zoom last changed, and whether it did since the last frame's look. */
  zoomed: number;
  changed: boolean;
  /** The frame that looks whether the zoom settled, or draws what is still stretched. */
  frame: number;
}

const groups = new Map<CameraStore, Group>();
const all = new Set<Drawing>();
const byArea = new WeakMap<Element, Drawing>();

/** What shows across, in beats; null before the view is laid out. */
function showing(s: CameraState) {
  const x = cull(s, s, { x: { margin: 0 } }).x;
  return x.to > x.from ? x : null;
}

/** Whether `d` holds the part of its clip that shows (`shows`, in beats; null: not known yet). One holding all its clip always does. */
function holdsShown(d: Drawing, shows: { from: number; to: number } | null) {
  if (!d.holds) return false;
  if (!shows || (d.holds[0] <= 0 && d.holds[1] >= d.length)) return true;
  const lo = Math.max(0, shows.from - d.start), hi = Math.min(d.length, shows.to - d.start);
  return hi <= lo || (d.holds[0] <= lo + 1e-9 && d.holds[1] >= hi - 1e-9);
}

/** Draw `d` again, now, at its area's size and the camera's zoom. */
function draw(d: Drawing) {
  d.stale = false;
  const size = d.size;
  if (!size) return;
  const s = d.cam?.getState() ?? null;
  const dpr = window.devicePixelRatio || 1;
  const k = d.length > 0 ? size.w / d.length : 0;
  // The part it holds: what shows and up to a view more each side, no wider than MAX_W device px.
  let [from, to] = [0, d.length];
  const shows = s && showing(s);
  if (shows && k > 0) {
    const wide = shows.to - shows.from;
    const margin = Math.max(0, Math.min(wide * CULL_MARGIN, (MAX_W / dpr / k - wide) / 2));
    from = Math.min(d.length, Math.max(0, shows.from - margin - d.start));
    to = Math.min(d.length, Math.max(from, shows.to + margin - d.start));
  } else to = Math.min(d.length, k > 0 ? MAX_W / dpr / k : 0);
  const w = (to - from) * k, h = size.h;
  const scale = w > 0 && h > 0 ? Math.min(dpr, MAX_W / w, MAX_H / h) : dpr;
  const bw = w > 0 ? Math.max(1, Math.round(w * scale)) : 0;
  const bh = h > 0 ? Math.max(1, Math.round(h * scale)) : 0;
  const c = d.canvas;
  const left = `${(from / d.length) * 100}%`, width = `${((to - from) / d.length) * 100}%`;
  if (c.style.left !== left) c.style.left = left;
  if (c.style.width !== width) c.style.width = width;
  if (c.width !== bw || c.height !== bh) {
    c.width = bw;
    c.height = bh;
  }
  const ctx = (d.ctx ??= c.getContext("2d", { willReadFrequently: true }));
  const drawn: Drawn = { zoom: s?.zoom ?? null, from, to, w, h, scale, ink: d.ink, notes: [], marks: [], line: [] };
  if (ctx && bw && bh) {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, bw, bh);
    // The area's px to the bitmap's: across from the part's left.
    const sx = bw / w, sy = bh / h, x0 = from * k;
    ctx.setTransform(sx, 0, 0, sy, -x0 * sx, 0);
    ctx.fillStyle = ctx.strokeStyle = d.ink;
    const pen: Pen = {
      note(x, y, nw, nh, alpha, mark) {
        ctx.globalAlpha = alpha;
        ctx.fillRect(x, y, nw, nh);
        drawn.notes.push({ x: x - x0, y, w: nw, h: nh, alpha });
        if (!mark) return;
        // 2 px wide at any zoom it is drawn at, down the note's middle.
        const mx = x + nw / 2 - FINE / 2;
        ctx.fillRect(mx, y, FINE, nh);
        drawn.marks.push({ x: mx - x0, y, w: FINE, h: nh, alpha });
      },
      curve(xy, foot) {
        if (!xy.length) return;
        ctx.beginPath();
        xy.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
        ctx.lineTo(xy[xy.length - 1][0], foot);
        ctx.lineTo(xy[0][0], foot);
        ctx.closePath();
        ctx.globalAlpha = 0.1;
        ctx.fill();
        ctx.beginPath();
        xy.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
        ctx.globalAlpha = 1;
        ctx.lineWidth = 1.5;
        ctx.lineJoin = "round";
        ctx.stroke();
        drawn.line = xy.map(([x, y]) => [x - x0, y]);
      },
    };
    const fine = d.fine !== null && s !== null && d.fine * s.zoom < FINE;
    d.paint(pen, k, h, from, to, fine);
    ctx.globalAlpha = 1;
  }
  countDraw(c.drawn ? "arrangement.canvas" : "arrangement.canvas.new");
  c.drawn = drawn;
  d.holds = [from, to];
}

/**
 * Every drawing still stretched by a zoom, drawn again at the start of a frame: after a zoom by the
 * hand, the first frame that finds no zoom for G and none since the frame before; after one by the
 * app, the frame after next (the observer has drawn the rest by then).
 */
function settle(g: Group, after = 0) {
  cancelAnimationFrame(g.frame);
  g.frame = requestAnimationFrame(() => {
    if (after > 0) return settle(g, after - 1);
    if (g.hand && (g.changed || performance.now() - g.zoomed < SWIPE_GAP)) {
      g.changed = false;
      return settle(g);
    }
    g.hand = false;
    for (const d of g.drawings) if (d.stale) draw(d);
  });
}

// Each drawing area's size, after layout: a new one draws; a zoom by the hand stretches it until it
// settles, unless what shows has left the part it holds.
const observer =
  typeof ResizeObserver === "undefined"
    ? null
    : new ResizeObserver((entries) => {
        for (const e of entries) {
          const d = byArea.get(e.target);
          if (!d) continue;
          d.size = { w: e.contentRect.width, h: e.contentRect.height };
          const g = d.cam && groups.get(d.cam);
          if (!d.holds || !g?.hand || !holdsShown(d, showing(d.cam!.getState()))) draw(d);
          else d.stale = true;
        }
      });

// A move of the screen's px a px (another screen, the page's zoom) draws every canvas again.
let dprQuery: MediaQueryList | null = null;
function watchDpr() {
  dprQuery?.removeEventListener("change", onDpr);
  dprQuery = typeof matchMedia === "undefined" ? null : matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  dprQuery?.addEventListener("change", onDpr);
}
function onDpr() {
  watchDpr();
  for (const d of all) draw(d);
}

function add(d: Drawing) {
  if (!all.size) watchDpr();
  all.add(d);
  byArea.set(d.area, d);
  observer?.observe(d.area);
  const cam = d.cam;
  if (!cam) return;
  let g = groups.get(cam);
  if (!g) {
    const group: Group = { drawings: new Set(), off: () => {}, hand: false, zoomed: 0, changed: false, frame: 0 };
    group.off = cam.subscribe((now, was) => {
      if (now.zoom !== was.zoom) {
        // Each drawing stretches with its clip. A zoom by the app: the observer draws each whose size
        // it tells, after this zoom's layout, and the frame after next those whose size no zoom
        // changes (a clip at its narrowest). A zoom by the hand: all once it settles.
        for (const x of group.drawings) x.stale = true;
        group.hand = now.by === "zoom";
        group.zoomed = performance.now();
        group.changed = true;
        settle(group, group.hand ? 0 : 1);
        return;
      }
      if (now.x === was.x && now.view === was.view) return;
      // A pan: only a drawing that holds part of its clip, once what shows leaves that part.
      const shows = showing(now);
      for (const x of group.drawings) if (x.holds && !holdsShown(x, shows)) draw(x);
    });
    groups.set(cam, (g = group));
  }
  g.drawings.add(d);
}

function remove(d: Drawing) {
  all.delete(d);
  byArea.delete(d.area);
  observer?.unobserve(d.area);
  if (!all.size) dprQuery?.removeEventListener("change", onDpr);
  const g = d.cam && groups.get(d.cam);
  if (!g) return;
  g.drawings.delete(d);
  if (g.drawings.size) return;
  g.off();
  cancelAnimationFrame(g.frame);
  groups.delete(d.cam!);
}

/**
 * A canvas for a clip from beat `start`, `length` beats long, drawn by `paint` in `ink` (`fine`: the
 * shortest note, for the marks). It is drawn again when one of `deps` changes, never on a render
 * that changes none. The arrangement's camera lends the zoom and the view.
 */
function useClipCanvas(start: number, length: number, fine: number | null, ink: string, paint: Paint, deps: unknown[]) {
  const cam = useCameraOf(ARRANGEMENT);
  const area = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const me = useRef<Drawing | null>(null);
  useLayoutEffect(() => {
    if (!area.current || !canvas.current) return;
    const d: Drawing = { canvas: canvas.current, area: area.current, cam, start, length, fine, ink, paint, size: null, holds: null, stale: false, ctx: null };
    me.current = d;
    add(d);
    return () => {
      remove(d);
      me.current = null;
    };
    // The rest is the next effect's: kept up to date there.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cam]);
  useLayoutEffect(() => {
    const d = me.current;
    if (!d) return;
    Object.assign(d, { start, length, fine, ink, paint });
    // Its first draw is the observer's, once its area is laid out.
    if (d.holds) draw(d);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [start, length, fine, ink, ...deps]);
  return { area, canvas };
}

/**
 * A mini piano roll of a clip's audible notes, over their own pitch range, on a canvas (VIEW-12). Each
 * note at its loudness by its own alpha; while the clip's shortest note is under 2 px at the zoom,
 * each note gets a 2 px mark down its middle (`marks`: "always" for a take while it records, whose
 * notes have always had them).
 */
export const Notes = memo(function Notes({
  notes,
  start,
  length,
  color,
  marks = "fine",
}: {
  notes: NoteClip["notes"];
  /** Where its clip starts in the song, in beats. */
  start: number;
  length: number;
  color: TrackColor;
  marks?: "fine" | "always";
}) {
  useRenderCount("arrangement.drawings");
  const fine = marks === "always" ? 0 : notes.length ? Math.min(...notes.map((n) => n.length)) : null;
  const { area, canvas } = useClipCanvas(
    start,
    length,
    fine,
    color.ink,
    (pen, k, h, from, to, fine) => {
      if (!notes.length) return;
      const lo = Math.min(...notes.map((n) => n.pitch));
      const hi = Math.max(...notes.map((n) => n.pitch));
      const rows = hi - lo + 1;
      // In hundredths of the area's height: few pitches stay thin and centred rather than filling it.
      const row = Math.min(100 / rows, 16);
      const top = (100 - rows * row) / 2;
      const y = h / 100;
      // A mark reaches 1 px past a thin note: the notes that far outside the part held too.
      const slack = FINE / Math.max(k, 1e-9);
      for (const n of notes) {
        if (n.start + n.length < from - slack || n.start > to + slack) continue;
        pen.note(n.start * k, (top + (hi - n.pitch) * row + row * 0.1) * y, n.length * k, row * 0.8 * y, 0.35 + 0.65 * n.velocity, fine);
      }
    },
    [notes],
  );
  return (
    <div ref={area} className="absolute left-0 w-full" style={AREA}>
      <canvas ref={canvas} data-drawing="notes" aria-hidden className="absolute top-0 h-full" />
    </div>
  );
});

/**
 * The automation curve, its line and the shade under it on a canvas (VIEW-12); its points marked over
 * it, round at any stretch. Values hold before the first point and after the last. The point `picked`
 * is `fg-strong` in a 2px white ring (taste.md §2.8).
 */
export const Curve = memo(function Curve({
  clip,
  scale,
  color,
  picked,
}: {
  clip: ControlClip;
  /** The lane's scale: where a value is up the clip, and the line through the points as it draws it (D3). */
  scale: LaneScale | null;
  color: TrackColor;
  picked: number;
}) {
  useRenderCount("arrangement.drawings");
  const level = (v: number) => scale?.level(v) ?? 0;
  const { points } = clip;
  const { area, canvas } = useClipCanvas(
    clip.start,
    clip.length,
    null,
    color.ink,
    (pen, k, h) => {
      const drawn = (scale?.path ?? line)(points, clip.length);
      pen.curve(
        drawn.map(([b, v]) => [b * k, (1 - level(v)) * h]),
        h,
      );
    },
    [points, scale],
  );
  return (
    <>
      <div ref={area} data-curve className="absolute left-0 w-full" style={AREA}>
        <canvas ref={canvas} data-drawing="curve" aria-hidden className="absolute top-0 h-full" />
      </div>
      {/* Circles stay round: drawn over the stretched drawing, not in it. */}
      <div aria-hidden className="absolute left-0 w-full pointer-events-none" style={AREA}>
        {points.map(([b, v], i) => (
          <span
            key={i}
            data-point={i}
            data-picked={i === picked || undefined}
            className={clsx("absolute size-1.5 -ml-[3px] -mt-[3px] rounded-full", i === picked && "bg-fg-strong ring-2 ring-white")}
            style={{ left: `${(b / clip.length) * 100}%`, top: `${(1 - level(v)) * 100}%`, background: i === picked ? undefined : color.ink }}
          />
        ))}
      </div>
    </>
  );
});
