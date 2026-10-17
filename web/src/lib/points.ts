// Automation drawn in place (8b.5, TL-6): what a press on an automation clip's curve hits, and
// the points a click, a drag or Draw mode's paint make of it. Pure: beats are the clip's own
// (0 at its start), values the lane's, and the drawing is `w`×`h` px with 0 at its top.

import { create } from "zustand";
import type { ClipRef } from "./graph";

export type Point = [number, number];

/** How close to the curve, or to a point, a press must be to edit it (taste.md TL-6). */
export const NEAR = 6;

/** The line drawn: the first value held from the clip's start, the last to its end. */
export function line(points: readonly Point[], length: number): Point[] {
  if (!points.length) return [];
  const first = points[0], last = points[points.length - 1];
  return [...(first[0] > 0 ? [[0, first[1]] as Point] : []), ...points, ...(last[0] < length ? [[length, last[1]] as Point] : [])];
}

/** Where `p` is drawn: px from the drawing's left and top. `level` maps a value to 0 (bottom) .. 1 (top). */
const at = (p: Point, length: number, level: (v: number) => number, w: number, h: number) => [(p[0] / length) * w, (1 - level(p[1])) * h];

/** The distance from (x, y) to the segment from a to b. */
function toSegment(x: number, y: number, [ax, ay]: number[], [bx, by]: number[]) {
  const dx = bx - ax, dy = by - ay;
  const t = dx || dy ? Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy))) : 0;
  return Math.hypot(x - (ax + t * dx), y - (ay + t * dy));
}

/**
 * What a press at (x, y) in the drawing hits: a point (its index, the nearest within `NEAR` px),
 * else the curve (within `NEAR` px of the line), else nothing: the clip's body.
 */
export function hit(points: readonly Point[], length: number, level: (v: number) => number, w: number, h: number, x: number, y: number) {
  let best = -1, near: number = NEAR;
  points.forEach((p, i) => {
    const [px, py] = at(p, length, level, w, h);
    const d = Math.hypot(x - px, y - py);
    if (d <= near) [best, near] = [i, d];
  });
  if (best >= 0) return { point: best };
  const drawn = line(points, length).map((p) => at(p, length, level, w, h));
  for (let i = 1; i < drawn.length; i++) if (toSegment(x, y, drawn[i - 1], drawn[i]) <= NEAR) return { curve: true as const };
  return null;
}

/** `points` with `p` added, in beat order (after any at its beat: a step, as drawn): the new list and where it went. */
export function added(points: readonly Point[], p: Point) {
  const i = points.findIndex((q) => q[0] > p[0]);
  const index = i < 0 ? points.length : i;
  return { points: [...points.slice(0, index), p, ...points.slice(index)], index };
}

/** `points` with point `i` at `p`, its beat kept between its neighbours' so the order stays. */
export function moved(points: readonly Point[], i: number, p: Point): Point[] {
  const lo = i > 0 ? points[i - 1][0] : -Infinity, hi = i < points.length - 1 ? points[i + 1][0] : Infinity;
  return points.map((q, k) => (k === i ? [Math.min(hi, Math.max(lo, p[0])), p[1]] : q));
}

/**
 * Draw mode's paint: `cells` (a beat on the grid, and the value painted there) in place of the
 * points between the first and the last of them.
 */
export function painted(points: readonly Point[], cells: ReadonlyMap<number, number>): Point[] {
  const beats = [...cells.keys()].sort((a, b) => a - b);
  if (!beats.length) return [...points];
  const lo = beats[0], hi = beats[beats.length - 1];
  const kept = points.filter((p) => p[0] < lo || p[0] > hi);
  return [...kept, ...beats.map((b) => [b, cells.get(b)!] as Point)].sort((a, b) => a[0] - b[0]);
}

/**
 * What a unit's values can be: a 0–1 amount and a −1–1 one stay in their range; a frequency, a
 * time, beats and a gain never go below 0. Others (dB, semitones, plain numbers) have no bound.
 */
const BOUNDS: Record<string, [number, number]> = { unit01: [0, 1], bipolar: [-1, 1], hz: [0, Infinity], seconds: [0, Infinity], beats: [0, Infinity], gain: [0, Infinity] };

/** `v` kept to what values of `unit` can be. */
export function bounded(unit: string | null | undefined, v: number) {
  const [lo, hi] = BOUNDS[unit ?? ""] ?? [-Infinity, Infinity];
  return Math.min(hi, Math.max(lo, v));
}

/** A value read off the drawing, rounded to a thousandth of the lane's range (a power of ten). */
export function rounded(v: number, range: number) {
  const step = 10 ** Math.floor(Math.log10(Math.max(range, 1e-9) / 1000));
  return Number((Math.round(v / step) * step).toPrecision(12));
}

/**
 * The automation point selected (⌫ deletes it), by its clip and its place in the clip's points; and
 * a clip's points as they are being edited, drawn in place of its own until the timeline shows the
 * edit (`landed`, the revision that does; Infinity until the server answers).
 */
interface Points {
  /** `at`: the point as picked, so an edit from elsewhere (an undo) that moves the points unpicks it. */
  picked: { clip: ClipRef; index: number; at: Point } | null;
  editing: { clip: ClipRef; points: Point[]; landed?: number } | null;
}

export const usePoints = create<Points>(() => ({ picked: null, editing: null }));

/** Which of `points` (a clip's, as drawn) is picked: -1 when none is, or the one picked has changed since. */
export function pickedIn(picked: Points["picked"], points: readonly Point[], editing: boolean) {
  if (!picked) return -1;
  const p = points[picked.index];
  return p && (editing || (p[0] === picked.at[0] && p[1] === picked.at[1])) ? picked.index : -1;
}
