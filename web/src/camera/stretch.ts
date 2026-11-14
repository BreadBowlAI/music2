// What a live zoom stretches (VIEW-5): the elements a view marks, by camera. While a zoom by the hand
// lasts, the camera writes one transform on each of them, a stretch around the content's origin, and
// lays the view out again only when the zoom ends (rig.ts). A view that marks nothing is laid out on
// each frame of a zoom, as before.
//
// The camera owns a marked element's `transform`, `transform-origin` and `will-change` while the
// stretch lasts: the view's own style must leave them alone. Marked elements are not nested in each
// other. What follows the zoom shown without being marked (a
// playhead that moves by its own transform, a strip outside the box) reads `stretchOf` or `shownAt`.

import { useCallback } from "react";
import { useCameraOf, type CameraRef, type CameraState, type CameraStore } from "./store";

/** Each camera's marked elements. */
const marked = new Map<CameraStore, Set<HTMLElement>>();
/** Who hears an element marked: the view at work on that camera, which stretches it at once while a live zoom lasts. */
const hearing = new Map<CameraStore, (el: HTMLElement) => void>();

/** The elements the camera `cam` stretches in a live zoom. */
export const markedOf = (cam: CameraStore): ReadonlySet<HTMLElement> => marked.get(cam) ?? new Set();

/** Hear each element marked for `cam` from now, until the function this returns is called. */
export function onMarked(cam: CameraStore, f: (el: HTMLElement) => void): () => void {
  hearing.set(cam, f);
  return () => void (hearing.get(cam) === f && hearing.delete(cam));
}

/** An element's stretch taken off: its style the view's own again, but for its layer while `keep`. */
export function unstretch(el: HTMLElement, keep = false) {
  el.style.transform = "";
  el.style.transformOrigin = "";
  if (!keep) el.style.willChange = "";
}

/**
 * A ref (React's, with its cleanup) that marks an element for the camera `ref` to stretch in a live
 * zoom (VIEW-5): a part of the view drawn by the zoom laid out (its lanes or grid, its ruler, a shade
 * over them). The same ref may mark many elements.
 */
export function useStretch(ref: CameraRef): (el: HTMLElement | null) => (() => void) | undefined {
  const cam = useCameraOf(ref);
  return useCallback(
    (el: HTMLElement | null) => {
      if (!el || !cam) return;
      let all = marked.get(cam);
      if (!all) marked.set(cam, (all = new Set()));
      all.add(el);
      hearing.get(cam)?.(el);
      const of = all;
      return () => {
        of.delete(el);
        if (!of.size && marked.get(cam) === of) marked.delete(cam);
        unstretch(el);
      };
    },
    [cam],
  );
}

/**
 * How a live zoom shows the view laid out (VIEW-5): scaled by `scale` each way around the content's
 * origin, then moved by `shift` px; null while none lasts. The shift takes the box from where it is
 * scrolled to where the zoom's end will scroll it, in whole px, so the stretch shows what that end
 * draws.
 */
export interface Stretch {
  scale: { x: number; y: number };
  shift: { x: number; y: number };
}

export function stretchOf(s: CameraState): Stretch | null {
  const l = s.laid;
  if (!l) return null;
  const k = s.zoom / l.zoom;
  return { scale: { x: s.stretch.x ? k : 1, y: s.stretch.y ? k : 1 }, shift: { x: l.x - Math.round(s.x), y: l.y - Math.round(s.y) } };
}

/**
 * Where the content's unit `u` across shows now, in px from the content's origin in the box as it is
 * laid out and scrolled: with a live zoom, where its stretch puts it. For what follows the zoom shown
 * by its own transform (the arrangement's playhead).
 */
export function shownAt(s: CameraState, u: number): number {
  return u * (s.stretch.x ? s.zoom : 1) + (stretchOf(s)?.shift.x ?? 0);
}
