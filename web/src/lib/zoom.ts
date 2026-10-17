// The timeline's zoom and whether it follows the playhead (Q8, Q11): kept per song in
// `.music2/view.json` (lib/local).

import { getLocal, setLocal, useLocal } from "./local";

/** Pixels a beat: 2 to 192, 24 at first; ⌘= and ⌘− multiply it by 1.5 (taste.md §3.2). */
export const ZOOM = { min: 2, max: 192, start: 24, step: 1.5 } as const;

export const clampZoom = (z: number) => Math.min(ZOOM.max, Math.max(ZOOM.min, z));

/** A kept zoom, or the start when it isn't one. */
const valid = (z: unknown) => (typeof z === "number" && Number.isFinite(z) ? clampZoom(z) : ZOOM.start);

/** The timeline's px a beat. */
export const useZoom = () => valid(useLocal<unknown>("zoom", ZOOM.start));
export const zoomNow = () => valid(getLocal<unknown>("zoom", ZOOM.start));

/** A zoom as kept: clamped, to a thousandth of a px. */
export const roundZoom = (z: number) => Math.round(clampZoom(z) * 1000) / 1000;

/** Set the zoom (as kept): what it is now. */
export function setZoom(z: number) {
  const to = roundZoom(z);
  setLocal("zoom", to);
  return to;
}

/** The zoom one step in (1) or out (-1) from `z`. */
export const stepped = (z: number, dir: 1 | -1) => clampZoom(dir > 0 ? z * ZOOM.step : z / ZOOM.step);

/** Following the playhead while it plays: on until turned off. */
export const useFollow = () => useLocal<unknown>("follow", true) !== false;
export const followNow = () => getLocal<unknown>("follow", true) !== false;
export const setFollow = (on: boolean) => setLocal("follow", on);
