// The timeline's zoom and whether it follows the playhead (Q8, Q11): kept per song in
// `.music2/view.json` (lib/local). The zoom is the arrangement's camera's (taste.md VIEW-1): each
// song's arrangement makes its camera at the zoom its view.json keeps (`kept`), and keeps view.json's
// copy of it as it changes (`keepZoom`).

import { cameraNow, controlsOf, onCamera, useCameraOf, useZoomOf, type CameraStore } from "../camera";
import { getLocal, setLocal, useLocal } from "./local";

/** The arrangement's camera. */
export const ARRANGEMENT = "arrangement";

/** Pixels a beat: 2 to 192, 24 at first; ⌘= and ⌘− multiply it by 1.5 (taste.md §3.2). */
export const ZOOM = { min: 2, max: 192, start: 24, step: 1.5 } as const;

export const clampZoom = (z: number) => Math.min(ZOOM.max, Math.max(ZOOM.min, z));

/** A kept zoom, or the start when it isn't one. */
const valid = (z: unknown) => (typeof z === "number" && Number.isFinite(z) ? clampZoom(z) : ZOOM.start);

/** The zoom the open song's view.json keeps: its arrangement's camera's when the arrangement makes it. */
export const kept = () => valid(getLocal<unknown>("zoom", ZOOM.start));

/** The timeline's px a beat: the arrangement's camera's, or view.json's while no arrangement is mounted. */
export function useZoom() {
  const cam = useCameraOf(ARRANGEMENT);
  const z = useZoomOf(ARRANGEMENT);
  const local = valid(useLocal<unknown>("zoom", ZOOM.start));
  return cam ? z : local;
}
export const zoomNow = () => cameraNow(ARRANGEMENT)?.getState().zoom ?? kept();

/** A zoom as kept: clamped, to a thousandth of a px. */
export const roundZoom = (z: number) => Math.round(clampZoom(z) * 1000) / 1000;

/** Set the zoom (as kept), the view where it is: what it is now. With no arrangement mounted, view.json's. */
export function setZoom(z: number) {
  const to = roundZoom(z);
  const c = controlsOf(ARRANGEMENT);
  if (c) c.to(undefined, undefined, to);
  else setLocal("zoom", to);
  return to;
}

/** Keep the arrangement's camera's zoom in view.json as it changes, until the function this returns is called. */
export const keepZoom = (cam: CameraStore) => onCamera(cam, (now, was) => now.zoom !== was.zoom && setLocal("zoom", now.zoom));

/** The zoom one step in (1) or out (-1) from `z`. */
export const stepped = (z: number, dir: 1 | -1) => clampZoom(dir > 0 ? z * ZOOM.step : z / ZOOM.step);

/** Following the playhead while it plays: on until turned off. */
export const useFollow = () => useLocal<unknown>("follow", true) !== false;
export const followNow = () => getLocal<unknown>("follow", true) !== false;
export const setFollow = (on: boolean) => setLocal("follow", on);
