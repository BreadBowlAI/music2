// Boxes laid along a camera's stretched way (VIEW-12): a clip, a gap between clips. Each is placed in
// px by the camera's zoom, written straight into its own style: a zoom moves every one with no React
// render, and changes no other element's style. (A CSS variable for the zoom did the same with no
// render, but a change to it reaches every element under it: the browser then worked out the style
// of the whole arrangement anew each frame, 56 ms of a zoom frame on the big song.)

import { useLayoutEffect, type RefObject } from "react";
import type { StoreApi } from "zustand";
import { cameraOf, type CameraState } from "./store";

interface Span {
  el: HTMLElement;
  from: number;
  /** Where it ends, in the content's units; null: only its start is placed. */
  to: number | null;
  inset: number;
  min: number;
}

/** Each camera's boxes, and how to stop hearing its zoom. */
const placed = new Map<StoreApi<CameraState>, { spans: Set<Span>; off: () => void }>();

function place(s: Span, zoom: number) {
  s.el.style.left = `${s.from * zoom + s.inset}px`;
  if (s.to !== null) s.el.style.width = `${Math.max((s.to - s.from) * zoom - 2 * s.inset, s.min)}px`;
}

/**
 * Place the element `ref` from `from` to `to` (the content's units: beats) along the camera `name`'s
 * stretched way, in px from where its parent starts: `inset` px in at each end, `min` px wide at the
 * least. The element's own style must leave `left` and `width` to this.
 */
export function useSpan(name: string, ref: RefObject<HTMLElement | null>, from: number, to: number | null, { inset = 0, min = 0 }: { inset?: number; min?: number } = {}) {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const cam = cameraOf(name);
    let group = placed.get(cam);
    if (!group) {
      const spans = new Set<Span>();
      const off = cam.subscribe((now, was) => {
        if (now.zoom !== was.zoom) for (const s of spans) place(s, now.zoom);
      });
      placed.set(cam, (group = { spans, off }));
    }
    const span: Span = { el, from, to, inset, min };
    const g = group;
    g.spans.add(span);
    place(span, cam.getState().zoom);
    return () => {
      g.spans.delete(span);
      if (g.spans.size) return;
      g.off();
      placed.delete(cam);
    };
  }, [name, from, to, inset, min]);
}
