// The timeline's zoom and whether it follows the playhead (Q8, Q11): kept per song in
// `.music2/view.json` (lib/local). The zoom is the arrangement's camera's (taste.md VIEW-1): the
// arrangement keeps view.json's copy of it as it changes (`keepZoom`).

import { cameraOf, controlsOf, onCamera, useZoomOf } from "../camera";
import { getLocal, setLocal, useLocal } from "./local";

/** The arrangement's camera. */
export const ARRANGEMENT = "arrangement";

/** Pixels a beat: 2 to 192, 24 at first; ⌘= and ⌘− multiply it by 1.5 (taste.md §3.2). */
export const ZOOM = { min: 2, max: 192, start: 24, step: 1.5 } as const;

export const clampZoom = (z: number) => Math.min(ZOOM.max, Math.max(ZOOM.min, z));

/** A kept zoom, or the start when it isn't one. */
const valid = (z: unknown) => (typeof z === "number" && Number.isFinite(z) ? clampZoom(z) : ZOOM.start);

/** The zoom view.json keeps: the arrangement's camera's when it is made. */
const kept = () => valid(getLocal<unknown>("zoom", ZOOM.start));

/** The timeline's px a beat. */
export const useZoom = () => useZoomOf(ARRANGEMENT, kept);
export const zoomNow = () => cameraOf(ARRANGEMENT, kept).getState().zoom;

/** A zoom as kept: clamped, to a thousandth of a px. */
export const roundZoom = (z: number) => Math.round(clampZoom(z) * 1000) / 1000;

/** Set the zoom (as kept), the view where it is: what it is now. */
export function setZoom(z: number) {
  const to = roundZoom(z);
  const c = controlsOf(ARRANGEMENT);
  if (c) c.to(undefined, undefined, to);
  else {
    cameraOf(ARRANGEMENT, kept).setState({ zoom: to });
    setLocal("zoom", to);
  }
  return to;
}

/** Keep the arrangement's zoom in view.json as it changes, until the function this returns is called. */
export const keepZoom = () => onCamera(ARRANGEMENT, (now, was) => now.zoom !== was.zoom && setLocal("zoom", now.zoom));

/** The zoom one step in (1) or out (-1) from `z`. */
export const stepped = (z: number, dir: 1 | -1) => clampZoom(dir > 0 ? z * ZOOM.step : z / ZOOM.step);

/** Following the playhead while it plays: on until turned off. */
export const useFollow = () => useLocal<unknown>("follow", true) !== false;
export const followNow = () => getLocal<unknown>("follow", true) !== false;
export const setFollow = (on: boolean) => setLocal("follow", on);
