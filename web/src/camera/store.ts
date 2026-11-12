// The cameras: one small zustand store each, by name (VIEW-1). Per-frame values (`x`, `y`) are read
// by subscription; hooks select only coarse ones (the zoom, a culling window's step), so moving the
// view renders nothing (VIEW-8).

import { useMemo } from "react";
import { createStore, useStore, type StoreApi } from "zustand";
import { NO_EDGES, cull, type Camera, type CullBy, type Shape, type Size, type Window } from "./math";

/** Who moved a camera last: the hand (a swipe, a glide), the app (Follow, Show, Fit…), the browser (Tab, a key, a scrollbar, a click), or a zoom. */
export type MovedBy = "hand" | "app" | "browser" | "zoom";

/** A camera, and what of its view's shape it keeps: its box's size, margins, origin and which ways it stretches. */
export interface CameraState extends Camera, Omit<Shape, "content"> {
  by: MovedBy;
}

const ZERO: Size = { w: 0, h: 0 };
const made = new Map<string, StoreApi<CameraState>>();

/** The camera `name`, made at `zoom` (or what `init` gives) the first time it is asked for. */
export function cameraOf(name: string, init?: () => number): StoreApi<CameraState> {
  let cam = made.get(name);
  if (!cam) {
    cam = createStore<CameraState>(() => ({
      x: 0,
      y: 0,
      zoom: init?.() ?? 1,
      view: ZERO,
      margins: NO_EDGES,
      origin: { x: 0, y: 0 },
      stretch: { x: true, y: true },
      by: "app",
    }));
    made.set(name, cam);
  }
  return cam;
}

/** Keep `cam` as the camera `name` (a view mounted again keeps the store its hooks read). */
export function adopt(name: string, cam: StoreApi<CameraState>) {
  if (made.get(name) !== cam) made.set(name, cam);
}

/** The cameras there are now, by name. */
export const cameraNames = () => [...made.keys()];

/** Whether the camera `name` is made. */
export const hasCamera = (name: string) => made.has(name);

/** Let go of the cameras whose names start with `prefix` (a view closed): the next ask makes them anew. */
export function forgetCameras(prefix: string) {
  for (const name of [...made.keys()]) if (name.startsWith(prefix)) made.delete(name);
}

/** Hear every change to the camera `name` (its position each frame it moves, too). Returns how to stop. */
export function onCamera(name: string, f: (now: CameraState, was: CameraState) => void): () => void {
  return cameraOf(name).subscribe(f);
}

/** The camera's zoom, for a view that draws by it: it renders when the zoom changes, never when the view only moves. */
export const useZoomOf = (name: string, init?: () => number) => useStore(cameraOf(name, init), (s) => s.zoom);

/** Which ways a culling window is wanted. */
export type Ways = "x" | "y" | "xy";

const key = (s: CameraState, ways: Ways, by: { x?: CullBy; y?: CullBy }) => {
  const w = cull(s, s, by);
  return `${s.zoom}|${s.view.w}|${s.view.h}|${s.margins.left}|${s.margins.top}|${s.margins.right}|${s.margins.bottom}|${ways.includes("x") ? w.x.step : ""}|${ways.includes("y") ? w.y.step : ""}`;
};

/**
 * What part of the content a view draws (VIEW-12): the window in view plus a margin, in the
 * content's units, each way asked for. It renders only when the window's step changes, the zoom, or
 * the box's size, never on a frame that stays in one step.
 */
export function useCull(name: string, ways: Ways = "xy", by: { x?: CullBy; y?: CullBy } = {}): { x: Window; y: Window } {
  const cam = cameraOf(name);
  // The margins and steps asked for, kept while they are the same numbers: a new object each render asks nothing new.
  const asked = JSON.stringify(by);
  const kept = useMemo(() => by, [asked]);
  useStore(cam, (s) => key(s, ways, kept));
  return cull(cam.getState(), cam.getState(), kept);
}
