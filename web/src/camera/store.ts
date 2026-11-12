// The cameras: one small zustand store each, by name (VIEW-1). Per-frame values (`x`, `y`) are read
// by subscription; hooks select only coarse ones (the zoom, a culling window's step), so moving the
// view renders nothing (VIEW-8).

import { startTransition, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createStore, useStore, type StoreApi } from "zustand";
import { NO_EDGES, cull, type Camera, type CullBy, type Shape, type Size, type Window } from "./math";

/** Who moved a camera last: the hand (a swipe, a glide), the app (Follow, Show, Fit…), the browser (Tab, a key, a scrollbar, a click), or a zoom. */
export type MovedBy = "hand" | "app" | "browser" | "zoom";

/** A camera, and what of its view's shape it keeps: its box's size, margins, origin and which ways it stretches. */
export interface CameraState extends Camera, Omit<Shape, "content"> {
  by: MovedBy;
  /**
   * The content's px the last zoom looked at, its view's width and height on: a view that keeps its
   * content at least this big keeps what a zoom held in place there, at its far end too.
   */
  asked: Size;
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
      asked: ZERO,
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

/** Any coarse part of the camera (its box's size, say), for a view that draws by it. Never `x` or `y`: those change every frame. */
export const useCameraState = <T,>(name: string, select: (s: CameraState) => T) => useStore(cameraOf(name), select);

/** Which ways a culling window is wanted. */
export type Ways = "x" | "y" | "xy";

/**
 * What part of the content a view draws (VIEW-12): the window in view plus a margin, in the
 * content's units, each way asked for. It renders the view only when that window is made anew (a pan
 * of a step, a zoom of ×1.5 out or ×2 in, the box's size), never on a frame that keeps it; and then as
 * a transition, which never holds up a frame of the swipe: the margin lets it lag. Only once what
 * shows has left the window is it drawn at once.
 */
export function useCull(name: string, ways: Ways = "xy", by: { x?: CullBy; y?: CullBy } = {}): { x: Window; y: Window } {
  const cam = cameraOf(name);
  // The margins and steps asked for, kept while they are the same numbers: a new object each render asks nothing new.
  const asked = JSON.stringify(by);
  const kept = useMemo(() => by, [asked]);
  const [shown, setShown] = useState(() => cull(cam.getState(), cam.getState(), kept));
  // The window asked for last: the next camera's checks keep it while its render waits.
  const latest = useRef(shown);
  useLayoutEffect(() => {
    const check = (s: CameraState) => {
      const was = latest.current;
      const w = cull(s, s, kept, was);
      if (!((ways.includes("x") && w.x !== was.x) || (ways.includes("y") && w.y !== was.y))) return;
      latest.current = w;
      // What shows, between the margins: still in the window drawn, the new one can wait.
      const shows = cull(s, s, { x: { margin: 0 }, y: { margin: 0 } });
      const inside = (a: Window, b: Window) => a.from <= b.from && a.to >= b.to;
      const waits = (!ways.includes("x") || inside(was.x, shows.x)) && (!ways.includes("y") || inside(was.y, shows.y));
      if (waits) startTransition(() => setShown(w));
      else setShown(w);
    };
    check(cam.getState());
    return cam.subscribe(check);
  }, [cam, kept, ways]);
  return shown;
}
