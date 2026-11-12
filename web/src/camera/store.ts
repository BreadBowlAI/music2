// The cameras: one small zustand store each, owned by the view that made it (VIEW-1). Per-frame
// values (`x`, `y`) are read by subscription; hooks select only coarse ones (the zoom, a culling
// window's step), so moving the view renders nothing (VIEW-8).
//
// A view makes its camera once, in its own state, and lends it under its name: to what it draws,
// through React's context, and to the rest of the app (the zoom buttons, agents, the suite) through
// the list of cameras mounted now. A view replaced by another of the same name in one commit (a
// song opened while another is open) never shares the old one's camera, or takes its zoom.

import { createContext, createElement, startTransition, useContext, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { createStore, useStore, type StoreApi } from "zustand";
import { NO_EDGES, cull, cullPlan, type Camera, type CullBy, type Shape, type Size, type Window } from "./math";

/**
 * Who moved a camera last: the hand (a swipe, a glide), the app (Follow, Show, Fit…), the browser
 * (Tab, a key, a scrollbar, a click), a zoom, or the box's own edge (`clamp`: the content got smaller
 * than where the camera looked).
 */
export type MovedBy = "hand" | "app" | "browser" | "zoom" | "clamp";

/** A camera, and what of its view's shape it keeps: its box's size, margins, origin and which ways it stretches. */
export interface CameraState extends Camera, Omit<Shape, "content"> {
  by: MovedBy;
  /**
   * The content's px the last zoom looked at, its view's width and height on: a view that keeps its
   * content at least this big keeps what a zoom held in place there, at its far end too.
   */
  asked: Size;
}

/** A camera's store. */
export type CameraStore = StoreApi<CameraState>;

const ZERO: Size = { w: 0, h: 0 };

/** A new camera at `zoom`, looking at the content's corner. */
export function makeCamera(zoom = 1): CameraStore {
  return createStore<CameraState>(() => ({
    x: 0,
    y: 0,
    zoom,
    view: ZERO,
    margins: NO_EDGES,
    origin: { x: 0, y: 0 },
    stretch: { x: true, y: true },
    by: "app",
    asked: ZERO,
  }));
}

// ---------------------------------------------------------------- the cameras mounted now

const mounted = new Map<string, CameraStore>();
const hearers = new Set<() => void>();
const tell = () => hearers.forEach((f) => f());

/** A view's camera, mounted as `name` (in the view's layout effect). */
export function mount(name: string, cam: CameraStore) {
  if (mounted.get(name) === cam) return;
  mounted.set(name, cam);
  tell();
}

/** A view's camera let go of: only while `name` is still that one (a view that replaced it keeps its own). */
export function unmount(name: string, cam: CameraStore) {
  if (mounted.get(name) !== cam) return;
  mounted.delete(name);
  tell();
}

/** The names of the cameras mounted now. */
export const cameraNames = () => [...mounted.keys()];

/** The camera mounted as `name` now, or null: for the app outside React (a handler, an agent's request). */
export const cameraNow = (name: string): CameraStore | null => mounted.get(name) ?? null;

const hear = (f: () => void) => {
  hearers.add(f);
  return () => void hearers.delete(f);
};
const deaf = () => () => {};

// ---------------------------------------------------------------- lent to what a view draws

const Lent = createContext<ReadonlyMap<string, CameraStore>>(new Map());

/** What a view draws, its camera lent under the camera's name: their hooks find it by that name. */
export function CameraScope({ camera, children }: { camera: { name: string; store: CameraStore }; children?: ReactNode }) {
  const above = useContext(Lent);
  const lent = useMemo(() => new Map([...above, [camera.name, camera.store]]), [above, camera.name, camera.store]);
  return createElement(Lent.Provider, { value: lent }, children);
}

/** A camera: by the name a view above lends it under (its `CameraScope`), or the view's own (what `useCamera` gave). */
export type CameraRef = string | { store: CameraStore };

/** What a hook reads where no camera has the name asked for: still, at zoom 1. */
const NONE = makeCamera();

/**
 * The camera `ref` names, at render: the one a view above lends under that name; else the one
 * mounted under it now (followed as that changes: a view mounted again, another song's); else null.
 */
export function useCameraOf(ref: CameraRef): CameraStore | null {
  const lent = useContext(Lent);
  const name = typeof ref === "string" ? ref : null;
  const mine = name === null ? null : (lent.get(name) ?? null);
  const far = useSyncExternalStore(name === null || mine ? deaf : hear, () => (name === null || mine ? null : (mounted.get(name) ?? null)));
  if (typeof ref !== "string") return ref.store;
  return mine ?? far;
}

const useStoreOf = (ref: CameraRef) => useCameraOf(ref) ?? NONE;

/** Hear every change to a view's camera (its position each frame it moves, too). Returns how to stop. */
export function onCamera(cam: { store: CameraStore } | CameraStore, f: (now: CameraState, was: CameraState) => void): () => void {
  return ("store" in cam ? cam.store : cam).subscribe(f);
}

/** The camera's zoom, for a view that draws by it: it renders when the zoom changes, never when the view only moves. */
export const useZoomOf = (ref: CameraRef) => useStore(useStoreOf(ref), (s) => s.zoom);

/** Any coarse part of the camera (its box's size, say), for a view that draws by it. Never `x` or `y`: those change every frame. */
export const useCameraState = <T,>(ref: CameraRef, select: (s: CameraState) => T) => useStore(useStoreOf(ref), select);

/** Which ways a culling window is wanted. */
export type Ways = "x" | "y" | "xy";

/**
 * What part of the content a view draws (VIEW-12): the window in view plus a margin, in the
 * content's units, each way asked for. It renders the view only when that window is made anew (a pan
 * of a step, a zoom of ×1.5 out or ×2 in, the box's size), never on a frame that keeps it; and then as
 * a transition, which never holds up a frame of the swipe: the margin lets it lag. Only once what
 * shows has left the window on screen (one asked for may not have landed yet) is it drawn at once.
 */
export function useCull(ref: CameraRef, ways: Ways = "xy", by: { x?: CullBy; y?: CullBy } = {}): { x: Window; y: Window } {
  const cam = useStoreOf(ref);
  // The margins and steps asked for, kept while they are the same numbers: a new object each render asks nothing new.
  const said = JSON.stringify(by);
  const kept = useMemo(() => by, [said]);
  const [shown, setShown] = useState(() => cull(cam.getState(), cam.getState(), kept));
  // The window on screen (set as the render that draws it lands), and the one asked for last.
  const committed = useRef(shown);
  const asked = useRef(shown);
  const of = useRef(cam);
  useLayoutEffect(() => {
    committed.current = shown;
  }, [shown]);
  useLayoutEffect(() => {
    if (of.current !== cam) {
      // Another camera (one mounted again under the name): a window of its own, at once.
      of.current = cam;
      asked.current = cull(cam.getState(), cam.getState(), kept);
      setShown(asked.current);
    }
    const check = (s: CameraState) => {
      const plan = cullPlan(s, s, kept, committed.current, asked.current, ways);
      if (plan.now === "keep") return;
      asked.current = plan.next;
      if (plan.now === "wait") startTransition(() => setShown(plan.next));
      else setShown(plan.next);
    };
    check(cam.getState());
    return cam.subscribe(check);
  }, [cam, kept, ways]);
  return shown;
}
