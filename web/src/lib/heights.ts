// Track and automation lane heights (Q10): kept per song in `.music2/view.json` (lib/local), as
// `heights: {"bass": 72, "bass.cutoff": 64}`. A track's notes rows all take its height; a lane is
// named `module.lane`. One at its usual height isn't kept.

import { getLocal, setLocal, useLocal } from "./local";
import { LANE_H, ROW_H } from "./layout";

/** A track's rows: 28 to 160 px in 4 px steps, 44 at first; ⌘↓ and ⌘↑ step through `steps`. */
export const TRACK_H = { min: 28, max: 160, start: ROW_H, steps: [28, 44, 72, 120] } as const;
/** An automation lane: 24 to 160 px, 32 at first. */
export const LANE_HS = { min: 24, max: 160, start: LANE_H } as const;
/** The step a dragged edge snaps to. */
const STEP = 4;

export type Heights = Record<string, number>;
const NONE: Heights = {};

const valid = (h: unknown): Heights => (h && typeof h === "object" && !Array.isArray(h) ? (h as Heights) : NONE);

export const useHeights = () => valid(useLocal<unknown>("heights", NONE));
export const heightsNow = () => valid(getLocal<unknown>("heights", NONE));

/** `h` snapped to the 4 px steps, between `min` and `max`. */
export const snapHeight = (h: number, { min, max }: { min: number; max: number }) => Math.min(max, Math.max(min, Math.round(h / STEP) * STEP));

const kept = (v: unknown, range: { min: number; max: number; start: number }) => (typeof v === "number" && Number.isFinite(v) ? snapHeight(v, range) : range.start);

/** Track `module`'s rows' height. */
export const trackHeight = (h: Heights, module: string) => kept(h[module], TRACK_H);
/** Automation lane `lane` of track `module`'s height. */
export const laneHeight = (h: Heights, module: string, lane: string) => kept(h[`${module}.${lane}`], LANE_HS);

/** Set heights by key (a track's, or `module.lane`): one at its usual height, or null, isn't kept. */
export function setHeights(changes: Record<string, number | null>) {
  const next = { ...heightsNow() };
  for (const [k, v] of Object.entries(changes)) {
    const usual = k.includes(".") ? LANE_HS.start : TRACK_H.start;
    if (v === null || v === usual) delete next[k];
    else next[k] = v;
  }
  setLocal("heights", Object.keys(next).length ? next : null);
}

/** ⌘↓ (1) and ⌘↑ (-1): the next of `steps` taller or shorter than `h`, or `h` at either end. */
export function stepHeight(h: number, dir: 1 | -1) {
  const steps: readonly number[] = TRACK_H.steps;
  return (dir > 0 ? steps.find((s) => s > h) : steps.findLast((s) => s < h)) ?? h;
}
