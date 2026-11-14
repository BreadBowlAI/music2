// Clips' mini drawings (taste.md VIEW-12; the zoom note, docs/designs/camera/zoom.md, §3): their notes,
// with their 2 px marks, and an automation curve's line and the shade under it, drawn on canvases in
// place of hundreds of SVG shapes. A zoom only stretches a canvas, which costs the browser almost
// nothing; it is drawn again, sharp at the screen's own pixels, once the zoom settles.
//
// - One canvas a lane (`LaneDrawing`) draws all its clips that sit still, each inside its box as the
//   box is placed. Chrome makes every canvas a layer of its own: one a clip made 416 layers for the
//   beat song's 36, and a frame with nothing new to draw 5 ms dearer. A clip that moves (dragged, or
//   a copy where it lands) and a take while it records draw on a canvas of their own, in their own
//   box (`Notes`, `Curve`): a few at a time.
// - Kept by what it draws: a canvas is drawn again only when that changes (the clips, their notes or
//   points, the colour, the size), and on a zoom: by the app (⌘=, Fit) in the frame it lands in; by
//   the hand (⌘-scroll, a pinch) once it settles (no zoom for G, `SWIPE_GAP`, nor in a whole frame,
//   looked at each frame's start), stretching with the view until then. A pan draws one again only
//   when what shows reaches past the part it holds, at once.
// - Bounded: a canvas holds only what shows and up to a view more each side (the views' culling
//   margin), and never a bitmap over MAX_W × MAX_H device px.
// - A lane's canvas is placed across by the zoom, as its clips' boxes are; its height, and a clip's
//   own canvas's size, come from one ResizeObserver (on the lane's box, on the clip's drawing area),
//   which tells them after layout and before paint. Nothing here reads the page's layout.
// - On the CPU (`willReadFrequently`): WebKit then paints it into the layer under it, not a layer of
//   its own (Chrome makes one either way).
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

/**
 * A clip's box (VIEW-12): `CLIP_INSET` px inside its lane at its top and foot; placed by the zoom 1 px
 * in at each end (`CLIP_END`) and `CLIP_MIN` px wide at the least; inside its `BORDER`, its drawing
 * `BODY_TOP` and `BODY_BOTTOM` px in from its top and foot.
 */
export const CLIP_INSET = 4;
export const CLIP_END = 1;
export const CLIP_MIN = 4;
export const BORDER = 1;
export const BODY_TOP = 4;
export const BODY_BOTTOM = 4;
/** A clip's drawing area inside its box. */
const AREA = { top: BODY_TOP, height: `calc(100% - ${BODY_TOP + BODY_BOTTOM}px)` };
/** Where the drawing of a clip in a lane starts down the lane, in px; and how tall it is in a lane `h` px tall. */
const AREA_TOP = CLIP_INSET + BORDER + BODY_TOP;
const areaHeight = (h: number) => Math.max(0, h - 2 * (CLIP_INSET + BORDER) - BODY_TOP - BODY_BOTTOM);
/**
 * Where the drawing of a clip from beat `start`, `length` long, is across its lane at zoom `z`: as its
 * box is placed, inside its border, on the screen's pixels (`px` of them a px) as the browser paints
 * a box: its ends rounded to them.
 */
const areaAcross = (start: number, length: number, z: number, px: number) => {
  const x = start * z + CLIP_END + BORDER, w = Math.max(length * z - 2 * CLIP_END, CLIP_MIN) - 2 * BORDER;
  const l = Math.round(x * px) / px;
  return { x: l, w: Math.round((x + w) * px) / px - l };
};

/** The biggest bitmap a canvas makes, in device px across and down (VIEW-12). */
export const MAX_W = 8192;
export const MAX_H = 512;

/** What one clip on a canvas looks like there, in px of the canvas's box. */
interface DrawnClip {
  /** Its beats in the song, and its px a beat inside its box. */
  start: number;
  length: number;
  k: number;
  notes: { x: number; y: number; w: number; h: number; alpha: number }[];
  /** Each note's 2 px mark, while the clip's shortest note is under 2 px. */
  marks: { x: number; y: number; w: number; h: number; alpha: number }[];
  /** An automation curve's vertices; its shade runs under them to the foot. */
  line: [number, number][];
}

/** What a canvas drew last, as it was then. */
export interface Drawn {
  /** The camera's zoom it was drawn at, in px a beat; null where no camera lends it one. */
  zoom: number | null;
  /** The beats of the song it holds. */
  from: number;
  to: number;
  /** Its box then, in px, and its bitmap's device px a px. */
  w: number;
  h: number;
  scale: number;
  /** The colour it drew in. */
  ink: string;
  clips: DrawnClip[];
}

/** What a drawing draws with, in px of its space (`Paint`), each shape also noted in its `drawn`. */
interface Pen {
  /**
   * The clip whose shapes come next: its beats in the song, `k` px a beat inside its box; on a lane's
   * canvas, its drawing area across (`x`, `w`), which they don't pass, as a box's own drawing doesn't.
   */
  clip(start: number, length: number, k: number, across?: { x: number; w: number }): void;
  /** A note's box in the ink at `alpha`; with `mark`, a 2 px mark down its middle too. */
  note(x: number, y: number, w: number, h: number, alpha: number, mark: boolean): void;
  /** A curve through `xy`: the shade under it to `foot` at 0.1, then its 1.5 px line. */
  curve(xy: [number, number][], foot: number): void;
}

/**
 * What a drawing draws, at zoom `z` (null: none lends one), `k` px a beat across its space and `h` px
 * tall, holding the song's beats `from` to `to`, on a bitmap of `px` device px a px. A lane's space
 * starts at the lane's left, at beat 0; a clip's own at its drawing area's left, at the clip's start.
 */
type Paint = (pen: Pen, z: number | null, k: number, h: number, from: number, to: number, px: number) => void;

interface Drawing {
  canvas: HTMLCanvasElement & { drawn?: Drawn };
  cam: CameraStore | null;
  /**
   * A lane's canvas, placed by the zoom, its height the clips' drawing area's in the lane's box
   * (`area`); else a clip's own, in its drawing area (`area`). The observer tells either's size.
   */
  lane: boolean;
  area: HTMLElement;
  /** The beats of the song it spans: a clip's; a lane's from 0 on. */
  start: number;
  length: number;
  /** Its space's size, in px: a lane's height; a clip's area, as the observer last told it. */
  size: { w: number; h: number } | null;
  ink: string;
  paint: Paint;
  /** The beats of the song it holds now; null until drawn. */
  holds: [number, number] | null;
  /** How to find, of those, the beats it has anything to draw in (a lane's clips), which its canvas covers. */
  narrow: ((from: number, to: number) => [number, number] | null) | null;
  /** A lane's canvas as its last draw placed it: px from the lane's left, at zoom `z`. */
  placed: { left: number; width: number; z: number } | null;
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

/** Whether `d` holds the part of what it spans that shows (`shows`, in beats; null: not known yet). */
function holdsShown(d: Drawing, shows: { from: number; to: number } | null) {
  if (!d.holds) return false;
  const end = d.start + d.length;
  if (!shows || (d.holds[0] <= d.start && d.holds[1] >= end)) return true;
  const lo = Math.max(d.start, shows.from), hi = Math.min(end, shows.to);
  return hi <= lo || (d.holds[0] <= lo + 1e-9 && d.holds[1] >= hi - 1e-9);
}

/** A lane's canvas placed at zoom `z`, as it was at its draw's: between draws, a zoom stretches it so. */
function placeLane(d: Drawing, z: number) {
  if (!d.placed) return;
  const by = z / d.placed.z;
  const left = `${d.placed.left * by}px`, width = `${d.placed.width * by}px`;
  if (d.canvas.style.left !== left) d.canvas.style.left = left;
  if (d.canvas.style.width !== width) d.canvas.style.width = width;
}

/** Draw `d` again, now, at its space's size and the camera's zoom. */
function draw(d: Drawing) {
  d.stale = false;
  const size = d.size;
  const s = d.cam?.getState() ?? null;
  const z = s?.zoom ?? null;
  const k = d.lane ? (z ?? 0) : size && d.length > 0 ? size.w / d.length : 0;
  if (!size || k <= 0) return;
  const dpr = window.devicePixelRatio || 1;
  // The part it holds: what shows and up to a view more each side, no wider than MAX_W device px.
  const end = d.start + d.length;
  let from = d.start, to = Math.min(end, d.start + MAX_W / dpr / k);
  const shows = s && showing(s);
  if (shows) {
    const wide = shows.to - shows.from;
    const margin = Math.max(0, Math.min(wide * CULL_MARGIN, (MAX_W / dpr / k - wide) / 2));
    from = Math.min(end, Math.max(d.start, shows.from - margin));
    to = Math.min(end, Math.max(from, shows.to + margin));
  }
  d.holds = [from, to];
  // Its canvas covers only the beats it has anything to draw in: none, an empty lane's.
  const [bf, bt] = d.narrow ? (d.narrow(from, to) ?? [from, from]) : [from, to];
  let x0 = (bf - d.start) * k, w = (bt - bf) * k;
  const h = size.h;
  const scale = w > 0 && h > 0 ? Math.min(dpr, MAX_W / w, MAX_H / h) : dpr;
  // A lane's canvas on the screen's own pixels: its left on one (the lane's box starts on one), its
  // width whole ones, so the browser never resamples it while it holds still.
  if (d.lane && w > 0) {
    const l = Math.floor(x0 * scale) / scale;
    w = Math.ceil((x0 + w) * scale) / scale - l;
    x0 = l;
  }
  const bw = w > 0 ? Math.max(1, Math.round(w * scale)) : 0;
  const bh = h > 0 ? Math.max(1, Math.round(h * scale)) : 0;
  const c = d.canvas;
  if (d.lane) {
    d.placed = { left: x0, width: w, z: k };
    placeLane(d, k);
  } else {
    const left = `${((bf - d.start) / d.length) * 100}%`, width = `${((bt - bf) / d.length) * 100}%`;
    if (c.style.left !== left) c.style.left = left;
    if (c.style.width !== width) c.style.width = width;
  }
  if (c.width !== bw || c.height !== bh) {
    c.width = bw;
    c.height = bh;
  }
  const ctx = (d.ctx ??= c.getContext("2d", { willReadFrequently: true }));
  const drawn: Drawn = { zoom: z, from, to, w, h, scale, ink: d.ink, clips: [] };
  if (ctx && bw && bh) {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, bw, bh);
    // Its space's px to the bitmap's, from the part's left.
    const sx = bw / w, sy = bh / h;
    ctx.setTransform(sx, 0, 0, sy, -x0 * sx, 0);
    ctx.fillStyle = ctx.strokeStyle = d.ink;
    let now: DrawnClip = { start: d.start, length: d.length, k, notes: [], marks: [], line: [] };
    const pen: Pen = {
      clip(start, length, ck, across) {
        drawn.clips.push((now = { start, length, k: ck, notes: [], marks: [], line: [] }));
        ctx.restore();
        ctx.save();
        if (!across) return;
        ctx.beginPath();
        ctx.rect(across.x, 0, across.w, h);
        ctx.clip();
      },
      note(x, y, nw, nh, alpha, mark) {
        ctx.globalAlpha = alpha;
        ctx.fillRect(x, y, nw, nh);
        now.notes.push({ x: x - x0, y, w: nw, h: nh, alpha });
        if (!mark) return;
        // 2 px wide at any zoom it is drawn at, down the note's middle.
        const mx = x + nw / 2 - FINE / 2;
        ctx.fillRect(mx, y, FINE, nh);
        now.marks.push({ x: mx - x0, y, w: FINE, h: nh, alpha });
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
        now.line = xy.map(([x, y]) => [x - x0, y]);
      },
    };
    ctx.save();
    d.paint(pen, z, k, h, from, to, sx);
    ctx.restore();
    ctx.globalAlpha = 1;
  }
  countDraw(c.drawn ? "arrangement.canvas" : "arrangement.canvas.new");
  c.drawn = drawn;
}

/**
 * Every drawing still stretched by a zoom, drawn again at the start of a frame: after a zoom by the
 * hand, the first frame that finds no zoom for G and none since the frame before; after one by the
 * app, the frame after next (by then the observer has drawn each clip's own whose size it changed).
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

// Each drawing's box, after layout: a clip's own drawing area, or the lane's box under a lane's
// canvas, of which only the height counts (its width follows the zoom, which places the canvas). A new
// one draws; a zoom by the hand stretches it until it settles, unless what shows has left the part it
// holds.
const observer =
  typeof ResizeObserver === "undefined"
    ? null
    : new ResizeObserver((entries) => {
        for (const e of entries) {
          const d = byArea.get(e.target);
          if (!d) continue;
          if (d.lane) {
            const h = areaHeight(e.contentRect.height);
            if (d.size?.h === h) continue;
            d.size = { w: 0, h };
            d.canvas.style.height = `${h}px`;
            draw(d);
            continue;
          }
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
      const shows = showing(now);
      if (now.zoom !== was.zoom) {
        // Each canvas stretches with the view: a lane's placed by the zoom now, a clip's own with its
        // box. A zoom by the app draws each lane's now and each clip's own as the observer tells its
        // size; a zoom by the hand, once it settles, unless what shows has left a lane's part.
        group.hand = now.by === "zoom";
        group.zoomed = performance.now();
        group.changed = true;
        for (const x of group.drawings) {
          x.stale = true;
          if (!x.lane) continue;
          if (!group.hand || !holdsShown(x, shows)) draw(x);
          else placeLane(x, now.zoom);
        }
        settle(group, group.hand ? 0 : 1);
        return;
      }
      if (now.x === was.x && now.view === was.view) return;
      // A pan: only a canvas whose part held no longer holds what shows.
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
 * A drawing on the canvas `canvas`, kept up to date: drawn again when one of `deps` changes, never on
 * a render that changes none. Without `area`, a lane's, in the lane's box its canvas sits in, covering
 * only the beats `narrow` finds anything in; else a clip's own, in its area `area`, from beat `start`,
 * `length` long. The arrangement's camera lends the zoom and the view; the observer, its size.
 */
function useDrawing(
  canvas: React.RefObject<HTMLCanvasElement | null>,
  area: React.RefObject<HTMLElement | null> | null,
  span: { start: number; length: number },
  ink: string,
  paint: Paint,
  deps: unknown[],
  narrow: Drawing["narrow"] = null,
) {
  const cam = useCameraOf(ARRANGEMENT);
  const me = useRef<Drawing | null>(null);
  useLayoutEffect(() => {
    const box = area ? area.current : (canvas.current?.parentElement ?? null);
    if (!canvas.current || !box) return;
    const d: Drawing = { canvas: canvas.current, cam, lane: !area, area: box, start: span.start, length: span.length, size: null, ink, paint, holds: null, narrow, placed: null, stale: false, ctx: null };
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
    Object.assign(d, { start: span.start, length: span.length, ink, paint, narrow });
    // Its first draw is the observer's, once its box is laid out.
    if (d.holds) draw(d);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [span.start, span.length, ink, cam, ...deps]);
}

/**
 * A clip's notes, from px `x` across, `k` px a beat, in a drawing area `h` px tall: a mini piano roll
 * of its audible notes over their own pitch range, each at its loudness by its own alpha; with
 * `fine`, a 2 px mark down each. Only the notes from beat `from` to `to` of the clip (a mark reaches
 * past a thin note: those near too).
 */
function paintNotes(pen: Pen, notes: NoteClip["notes"], x: number, k: number, h: number, from: number, to: number, fine: boolean) {
  if (!notes.length) return;
  let lo = Infinity, hi = -Infinity;
  for (const n of notes) {
    lo = Math.min(lo, n.pitch);
    hi = Math.max(hi, n.pitch);
  }
  const rows = hi - lo + 1;
  // In hundredths of the area's height: few pitches stay thin and centred rather than filling it.
  const row = Math.min(100 / rows, 16);
  const top = (100 - rows * row) / 2;
  const y = h / 100;
  const slack = FINE / Math.max(k, 1e-9);
  for (const n of notes) {
    if (n.start + n.length < from - slack || n.start > to + slack) continue;
    pen.note(x + n.start * k, (top + (hi - n.pitch) * row + row * 0.1) * y, n.length * k, row * 0.8 * y, 0.35 + 0.65 * n.velocity, fine);
  }
}

/** Whether notes whose shortest is `shortest` beats get 2 px marks at zoom `z`. */
const fineAt = (shortest: number | null, z: number | null) => shortest !== null && z !== null && shortest * z < FINE;
const shortestOf = (notes: NoteClip["notes"]) => (notes.length ? Math.min(...notes.map((n) => n.length)) : null);

/** A curve through `points` over a clip `length` beats long, from px `x` across, `k` px a beat, in an area `h` px tall, on the lane's scale. */
function paintCurve(pen: Pen, points: ControlClip["points"], length: number, x: number, k: number, h: number, scale: LaneScale | null) {
  const level = (v: number) => scale?.level(v) ?? 0;
  const drawn = (scale?.path ?? line)(points, length);
  pen.curve(
    drawn.map(([b, v]) => [x + b * k, (1 - level(v)) * h]),
    h,
  );
}

/** A clip a lane's canvas draws: a notes clip's audible notes, or an automation clip's points. */
export type StillClip = { start: number; length: number; notes: NoteClip["notes"] } | { start: number; length: number; points: ControlClip["points"] };

/**
 * The canvas of a lane (VIEW-12), in the lane's box: it draws each of `clips` (those that sit still)
 * inside its box, as the zoom places the box. Over the clips' bodies and borders, under what a clip puts over its drawing
 * (a curve's points, the hatching): `z-[1]`, which they go over with `z-[2]`. It takes no pointer.
 */
export const LaneDrawing = memo(function LaneDrawing({
  clips,
  color,
  scale,
}: {
  clips: readonly StillClip[];
  color: TrackColor;
  /** An automation lane's scale (D3). */
  scale: LaneScale | null;
}) {
  useRenderCount("arrangement.drawings");
  const canvas = useRef<HTMLCanvasElement>(null);
  // Each clip's shortest note, once a clip.
  const shortest = useRef(new WeakMap<object, number | null>());
  const shortOf = (c: StillClip & { notes: NoteClip["notes"] }) => {
    if (!shortest.current.has(c)) shortest.current.set(c, shortestOf(c.notes));
    return shortest.current.get(c)!;
  };
  useDrawing(
    canvas,
    null,
    { start: 0, length: Infinity },
    color.ink,
    (pen, z, _k, ah, from, to, px) => {
      if (z === null) return;
      for (const c of clips) {
        if (c.start + c.length < from || c.start > to) continue;
        const area = areaAcross(c.start, c.length, z, px);
        const k = area.w / c.length;
        pen.clip(c.start, c.length, k, area);
        if ("notes" in c) paintNotes(pen, c.notes, area.x, k, ah, from - c.start, to - c.start, fineAt(shortOf(c), z));
        else paintCurve(pen, c.points, c.length, area.x, k, ah, scale);
      }
    },
    [clips, scale],
    // The beats from the first clip in `from`..`to` to the end of the last.
    (from, to) => {
      let a = Infinity, b = -Infinity;
      for (const c of clips)
        if (c.start + c.length >= from && c.start <= to) {
          a = Math.min(a, c.start);
          b = Math.max(b, c.start + c.length);
        }
      return a < b ? [Math.max(from, a), Math.min(to, b)] : null;
    },
  );
  // In the lane's box, as its clips: their drawing areas' band, its height the observer's.
  return <canvas ref={canvas} data-drawing="lane" aria-hidden className="absolute z-[1] pointer-events-none" style={{ top: AREA_TOP }} />;
});

/**
 * A clip's own mini piano roll, on a canvas in its box (VIEW-12): for a clip that moves (dragged, or
 * a copy where it lands) and a take while it records; a lane's canvas draws the clips that sit still.
 * While the clip's shortest note is under 2 px at the zoom, each note gets a 2 px mark (`marks`:
 * "always" for a take, whose notes have always had them).
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
  const area = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  useDrawing(
    canvas,
    area,
    { start, length },
    color.ink,
    (pen, z, k, h, from, to) => {
      pen.clip(start, length, k);
      paintNotes(pen, notes, 0, k, h, from - start, to - start, marks === "always" || fineAt(shortestOf(notes), z));
    },
    [notes, marks],
  );
  return (
    <div ref={area} className="absolute left-0 w-full" style={AREA}>
      <canvas ref={canvas} data-drawing="notes" aria-hidden className="absolute top-0 h-full" />
    </div>
  );
});

/** A clip's own automation curve, its line and the shade under it, on a canvas in its box: for a clip that moves. */
const CurveCanvas = memo(function CurveCanvas({ clip, scale, color }: { clip: ControlClip; scale: LaneScale | null; color: TrackColor }) {
  const area = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  useDrawing(canvas, area, { start: clip.start, length: clip.length }, color.ink, (pen, _z, k, h) => {
    pen.clip(clip.start, clip.length, k);
    paintCurve(pen, clip.points, clip.length, 0, k, h, scale);
  }, [clip.points, scale]);
  return (
    <div ref={area} className="absolute left-0 w-full" style={AREA}>
      <canvas ref={canvas} data-drawing="curve" aria-hidden className="absolute top-0 h-full" />
    </div>
  );
});

/**
 * An automation clip's points over its curve, round at any stretch; the curve itself on its lane's
 * canvas, or on one of its own while it moves (`own`). Values hold before the first point and after
 * the last. The point `picked` is `fg-strong` in a 2px white ring (taste.md §2.8).
 */
export const Curve = memo(function Curve({
  clip,
  scale,
  color,
  picked,
  own = false,
}: {
  clip: ControlClip;
  /** The lane's scale: where a value is up the clip, and the line through the points as it draws it (D3). */
  scale: LaneScale | null;
  color: TrackColor;
  picked: number;
  own?: boolean;
}) {
  useRenderCount("arrangement.drawings");
  const level = (v: number) => scale?.level(v) ?? 0;
  return (
    <>
      {own && <CurveCanvas clip={clip} scale={scale} color={color} />}
      {/* Over the lane's canvas (`z-[2]`), in the curve's drawing area. */}
      <div aria-hidden data-curve className="absolute left-0 w-full z-[2] pointer-events-none" style={AREA}>
        {clip.points.map(([b, v], i) => (
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
