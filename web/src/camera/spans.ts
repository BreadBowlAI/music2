// Boxes laid along a camera's stretched way (VIEW-12): a clip, a gap between clips. Each is placed in
// px by the camera's zoom, written straight into its own style: a zoom moves every one with no React
// render, and changes no other element's style. (A CSS variable for the zoom did the same with no
// render, but a change to it reaches every element under it: the browser then worked out the style
// of the whole arrangement anew each frame, 56 ms of a zoom frame on the big song.)
//
// Each box's ends lie on the screen's own pixels (`spanBox`): what draws over a box (the clips'
// canvases) finds it with the same function, so the two never disagree by a part of a pixel.

import { useLayoutEffect, type RefObject } from "react";
import { laidOf, useCameraOf, type CameraRef, type CameraStore } from "./store";

interface Span {
  el: HTMLElement;
  from: number;
  /** Where it ends, in the content's units; null: only its start is placed. */
  to: number | null;
  inset: number;
  min: number;
}

/** Under this many px, what a box draws is too thin to see unless marked: the clips' 2 px marks (VIEW-12). */
export const FINE = 2;

/** `px` on the screen's own pixels: rounded to the nearest of them (`devicePixelRatio` a px). */
export const onPixels = (px: number) => {
  const dpr = (typeof window === "undefined" ? 1 : window.devicePixelRatio) || 1;
  return Math.round(px * dpr) / dpr;
};

/**
 * Where a box from `from` to `to` (the content's units; `to` null: only its start) lies along a
 * camera's stretched way at `zoom`, in px from where its parent starts: `inset` px in at each end,
 * `min` px wide at the least, its start and its width on the screen's pixels (each rounded, as the
 * browser rounds a box's place and size when it paints one).
 */
export function spanBox(from: number, to: number | null, zoom: number, inset = 0, min = 0): { left: number; width: number | null } {
  const left = onPixels(from * zoom + inset);
  if (to === null) return { left, width: null };
  return { left, width: onPixels(Math.max((to - from) * zoom - 2 * inset, min)) };
}

/** Each camera's boxes, and how to stop hearing its zoom. */
const placed = new Map<CameraStore, { spans: Set<Span>; off: () => void }>();

// The screen's px a px changed (another screen, the page's zoom): every box onto its new pixels.
let pixels: MediaQueryList | null = null;
function watchPixels() {
  if (typeof matchMedia === "undefined") return;
  pixels?.removeEventListener("change", newPixels);
  pixels = matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  pixels.addEventListener("change", newPixels);
}
function newPixels() {
  watchPixels();
  for (const [cam, g] of placed) for (const s of g.spans) place(s, laidOf(cam.getState()).zoom);
}

function place(s: Span, zoom: number) {
  const box = spanBox(s.from, s.to, zoom, s.inset, s.min);
  s.el.style.left = `${box.left}px`;
  if (box.width !== null) s.el.style.width = `${box.width}px`;
}

/**
 * Place the element `ref` from `from` to `to` (the content's units: beats) along the camera's
 * stretched way, in px from where its parent starts: `inset` px in at each end, `min` px wide at the
 * least, each end on the screen's pixels (`spanBox`). The element's own style must leave `left` and
 * `width` to this.
 */
export function useSpan(
  ref: CameraRef,
  box: RefObject<HTMLElement | null>,
  from: number,
  to: number | null,
  { inset = 0, min = 0 }: { inset?: number; min?: number } = {},
) {
  // The camera at render: a view above lends it (never looked up later, by name, in the effect).
  const cam = useCameraOf(ref);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || !cam) return;
    let group = placed.get(cam);
    if (!group) {
      if (!placed.size) watchPixels();
      const spans = new Set<Span>();
      // By the zoom laid out: a live zoom stretches the boxes as they are (VIEW-5).
      const off = cam.subscribe((now, was) => {
        const zoom = laidOf(now).zoom;
        if (zoom !== laidOf(was).zoom) for (const s of spans) place(s, zoom);
      });
      placed.set(cam, (group = { spans, off }));
    }
    const span: Span = { el, from, to, inset, min };
    const g = group;
    g.spans.add(span);
    place(span, laidOf(cam.getState()).zoom);
    return () => {
      g.spans.delete(span);
      if (g.spans.size) return;
      g.off();
      placed.delete(cam);
    };
  }, [cam, from, to, inset, min]);
}
