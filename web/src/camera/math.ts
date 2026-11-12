// The camera's pure functions (the camera note's §3): each takes a camera and the view's shape and
// gives a new camera, or a conversion. No DOM, no state: the `camera` suite checks each against
// known numbers.

import { CULL_MARGIN, CULL_STEP } from "./numbers";

/** Where a view looks, and its zoom (VIEW-1). `x` and `y` are its box's scroll position, in the content's px at this zoom, with fractions. */
export interface Camera {
  x: number;
  y: number;
  /** The view's own unit: px a beat in the arrangement, Notes and the beat grid; the scale in the Graph. */
  zoom: number;
}

export interface Size {
  w: number;
  h: number;
}

/** What covers each edge of the box and stays there as it scrolls: a sticky ruler, names, keys, a Song row. */
export interface Edges {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export const NO_EDGES: Edges = { left: 0, top: 0, right: 0, bottom: 0 };

/** A view's shape: what the camera's functions need to know of its box and content. */
export interface Shape {
  /** The box's size on screen (its client size). */
  view: Size;
  /** The content's size at the camera's zoom (the box's scroll size). */
  content: Size;
  margins: Edges;
  /** Where the part of the content the zoom stretches starts, in px (the headers' width across). */
  origin: { x: number; y: number };
  /** Which ways the zoom stretches the content. */
  stretch: { x: boolean; y: boolean };
}

/** A span of the content, [from, to], in its px. */
export type Span = [number, number];

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** How far the camera can go each way: the content less the view. */
export const room = (s: Shape) => ({ x: Math.max(0, s.content.w - s.view.w), y: Math.max(0, s.content.h - s.view.h) });

/** The camera at (x, y), stopped at the content's edges (VIEW-6). */
export function to(c: Camera, x: number, y: number, s: Shape): Camera {
  const r = room(s);
  return { ...c, x: clamp(x, 0, r.x), y: clamp(y, 0, r.y) };
}

/** The camera moved by (dx, dy), stopped at the edges. */
export const move = (c: Camera, dx: number, dy: number, s: Shape): Camera => to(c, c.x + dx, c.y + dy, s);

/** A point in the box (px from its top-left corner) as the content's units: beats (or the Graph's px) where the zoom stretches, px from the origin elsewhere. */
export function toContent(c: Camera, p: { x: number; y: number }, s: Pick<Shape, "origin" | "stretch">) {
  const cx = c.x + p.x - s.origin.x, cy = c.y + p.y - s.origin.y;
  return { x: s.stretch.x ? cx / c.zoom : cx, y: s.stretch.y ? cy / c.zoom : cy };
}

/** A point in the content's units as a point in the box. */
export function toBox(c: Camera, u: { x: number; y: number }, s: Pick<Shape, "origin" | "stretch">) {
  const cx = s.origin.x + (s.stretch.x ? u.x * c.zoom : u.x), cy = s.origin.y + (s.stretch.y ? u.y * c.zoom : u.y);
  return { x: cx - c.x, y: cy - c.y };
}

/**
 * The camera at `zoom`, what is under the box point `at` staying under it (VIEW-5). A point over a
 * margin (a sticky header) counts as that margin's inner edge. Stopped at 0; the far edge is the
 * writer's, once the view has drawn the new zoom.
 */
export function zoomAround(c: Camera, at: { x: number; y: number }, zoom: number, s: Omit<Shape, "content">): Camera {
  const p = { x: clamp(at.x, s.margins.left, Math.max(s.margins.left, s.view.w - s.margins.right)), y: clamp(at.y, s.margins.top, Math.max(s.margins.top, s.view.h - s.margins.bottom)) };
  const u = toContent(c, p, s);
  const z = { ...c, zoom };
  const b = toBox(z, u, s);
  return { x: Math.max(0, z.x + b.x - p.x), y: Math.max(0, z.y + b.y - p.y), zoom };
}

/** How `reveal` places what it shows: the least move, or in the middle, or at the start. */
export type Align = "nearest" | "center" | "start";

/**
 * The least move that shows `box` (spans of the content's px, either way left out to keep it) clear
 * of the margins, `pad` px around it; one bigger than the clear part shows its start.
 */
export function reveal(c: Camera, box: { x?: Span; y?: Span }, s: Shape, { pad = 0, align = "nearest" }: { pad?: number; align?: Align } = {}): Camera {
  const axis = (at: number, span: Span | undefined, lo: number, hi: number, view: number) => {
    if (!span) return at;
    const a = span[0] - pad, b = span[1] + pad;
    const first = a - lo, last = b - (view - hi), clear = view - lo - hi;
    if (b - a > clear || align === "start") return first;
    if (align === "center") return (first + last) / 2;
    return at > first ? first : at < last ? last : at;
  };
  const m = s.margins;
  return to(c, axis(c.x, box.x, m.left, m.right, s.view.w), axis(c.y, box.y, m.top, m.bottom, s.view.h), s);
}

/**
 * Zoom so the units [from, to] across fill the clear part of the view, `pad` px each side (Z, Fit).
 * `zoomOf` clamps or rounds the zoom first; the span then starts at the pad.
 */
export function fit(c: Camera, [a, b]: [number, number], s: Omit<Shape, "content">, { pad = 0, zoomOf = (z: number) => z }: { pad?: number; zoomOf?: (z: number) => number } = {}): Camera {
  const clear = s.view.w - s.margins.left - s.margins.right - 2 * pad;
  const zoom = zoomOf(clear / Math.max(b - a, 1e-9));
  return { x: Math.max(0, s.origin.x + a * zoom - s.margins.left - pad), y: c.y, zoom };
}

/** One way of a culling window: the units drawn, [from, to], and its step's number (it changes only when the step does). */
export interface Window {
  from: number;
  to: number;
  step: number;
}

/** A way's margin and step, in px of the box: by default `CULL_MARGIN` and `CULL_STEP` of the clear view. */
export interface CullBy {
  margin?: number;
  step?: number;
}

/**
 * What part of the content is in view plus a margin, in its units, each way (VIEW-12). The window
 * moves in steps: it is the same while the camera stays in one step, so a view that draws only it
 * redraws only when the camera crosses a step. A box not laid out yet (no size) shows nothing.
 */
export function cull(c: Camera, s: Omit<Shape, "content">, by: { x?: CullBy; y?: CullBy } = {}): { x: Window; y: Window } {
  const axis = (at: number, lo: number, hi: number, view: number, origin: number, stretch: boolean, b: CullBy = {}): Window => {
    const clear = view - lo - hi;
    if (clear <= 0) return { from: 0, to: 0, step: 0 };
    const margin = b.margin ?? clear * CULL_MARGIN;
    const size = Math.max(1, b.step ?? clear * CULL_STEP);
    const step = Math.floor(at / size);
    // In the content's px: the clear part starts `lo` in from the camera.
    const from = step * size - margin + lo, to = (step + 1) * size + clear + margin + lo;
    const unit = (px: number) => (stretch ? (px - origin) / c.zoom : px - origin);
    return { from: unit(from), to: unit(to), step };
  };
  const m = s.margins;
  return {
    x: axis(c.x, m.left, m.right, s.view.w, s.origin.x, s.stretch.x, by.x),
    y: axis(c.y, m.top, m.bottom, s.view.h, s.origin.y, s.stretch.y, by.y),
  };
}

/** Whether [start, end] of the content's units meets a window. */
export const within = (w: Window, start: number, end = start) => end >= w.from && start <= w.to;
