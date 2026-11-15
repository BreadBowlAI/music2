// The drawn zoom (VIEW-5, VIEW-13): a view that draws its zoom by the hand itself. While a pinch or a
// run of ⌘-scroll lasts, the camera shows the view's picture over it and has the view paint it,
// sharp at the zoom shown, in each frame the zoom moves. The page under it stays as it was laid out.
// When the zoom ends, the view is laid out once, at the zoom shown, and the picture hides in the
// render that lays it out (rig.ts): no frame shows the one without the other. A view that doesn't
// draw its zoom is laid out at the zoom shown each frame.
//
// The camera owns the picture's life: when it shows, when it paints, when it hides, and what a press
// on it does. The view owns what is drawn: a scene, read as the zoom begins from what the view renders
// from, and a paint function of the scene and the camera shown.

import { useLayoutEffect, useMemo, useRef, type RefObject } from "react";
import { useCameraOf, type CameraRef, type CameraState, type CameraStore } from "./store";

/** What a view draws its zoom with. */
export interface Picture<S = unknown> {
  /** What holds the picture, over the view: shown (its `visibility`) only while a zoom by the hand lasts. */
  el: HTMLElement;
  /** What it draws, read from what the view renders from: as a zoom by the hand begins, and again when the view says it changed. */
  scene(): S;
  /** The picture of `scene` at the camera shown `s`. What it returns is what it drew, for the suites (`face.picture`). */
  paint(scene: S, s: CameraState): unknown;
}

/** A camera's picture at work: its scene while it shows, and what it drew last. */
interface Drawer {
  picture: Picture;
  scene: unknown;
  shown: boolean;
  drawn: unknown;
  /** How many times it painted. */
  paints: number;
  /** Why it couldn't paint last, and when (`performance.now()`): the zoom was laid out each frame instead. */
  failed: { why: string; at: number } | null;
  /** The camera suite's paint that throws (`face.pictureFails`). */
  failing: boolean;
  /** Asked to paint again (the view changed, or moved by itself), and to read its scene first. */
  dirty: boolean;
  reread: boolean;
}

const drawers = new Map<CameraStore, Drawer>();
/** Each camera's way to have its frame come: a picture asked to paint again paints in it, at most once a frame. */
const wakers = new Map<CameraStore, () => void>();

/** `cam`'s rig hears a picture that asks to paint again, and paints it in its next frame (`paintPicture`). */
export function onPictureWants(cam: CameraStore, f: () => void): () => void {
  wakers.set(cam, f);
  return () => void (wakers.get(cam) === f && wakers.delete(cam));
}

/** Whether the view of `cam` draws its zoom (VIEW-5). */
export const drawsZoom = (cam: CameraStore) => drawers.has(cam);

/** Whether the picture of `cam`'s view shows now. */
export const pictureShown = (cam: CameraStore) => !!drawers.get(cam)?.shown;

function paintOn(d: Drawer, s: CameraState) {
  if (d.failing) throw new Error("the camera suite's paint that throws");
  d.drawn = d.picture.paint(d.scene, s);
  d.paints++;
  d.dirty = false;
}

/**
 * Paint `cam`'s picture at the camera shown, showing it first, its scene read then, if it was hidden.
 * Returns false if it couldn't (reading or painting threw: a browser without a canvas feature it
 * uses): then it is hidden, why is kept (`pictureOf`), and the camera lays the zoom out instead, so
 * the view never freezes (VIEW-5).
 */
export function showPicture(cam: CameraStore): boolean {
  const d = drawers.get(cam);
  if (!d) return false;
  try {
    if (!d.shown || d.reread) d.scene = d.picture.scene();
    d.reread = false;
    paintOn(d, cam.getState());
  } catch (error) {
    d.failed = { why: error instanceof Error ? error.message : String(error), at: performance.now() };
    hidePicture(cam);
    return false;
  }
  if (!d.shown) {
    d.shown = true;
    d.picture.el.style.visibility = "visible";
  }
  return true;
}

/**
 * Paint `cam`'s picture again if the view asked (`redraw`) since it last painted: in the camera's
 * frame, so it paints at most once a frame however often it is asked (VIEW-8). A paint that throws
 * leaves it as it was: the zoom's next frame paints it again, or lays the zoom out if it can't.
 */
export function paintPicture(cam: CameraStore) {
  const d = drawers.get(cam);
  if (!d?.shown || !d.dirty) return;
  try {
    if (d.reread) d.scene = d.picture.scene();
    d.reread = false;
    paintOn(d, cam.getState());
  } catch (error) {
    d.dirty = false;
    d.failed = { why: error instanceof Error ? error.message : String(error), at: performance.now() };
  }
}

/** Whether the client point (`x`, `y`) is on `cam`'s picture as it shows: inside what its element holds (a press there only ends the zoom, VIEW-5). */
export function onPicture(cam: CameraStore, x: number, y: number): boolean {
  const d = drawers.get(cam);
  if (!d?.shown) return false;
  for (const c of d.picture.el.children) {
    const r = c.getBoundingClientRect();
    if (x >= r.left && x < r.right && y >= r.top && y < r.bottom) return true;
  }
  return false;
}

/** Hide `cam`'s picture: the page laid out at the zoom it showed shows now (in this same task). */
export function hidePicture(cam: CameraStore) {
  const d = drawers.get(cam);
  if (!d?.shown) return;
  d.shown = false;
  d.scene = null;
  d.dirty = d.reread = false;
  d.picture.el.style.visibility = "hidden";
}

/** What `cam`'s picture drew last, whether it shows, and why it last couldn't paint: for the suites (`face.picture`) and the swipe probe's report. */
export function pictureOf(cam: CameraStore): { shown: boolean; paints: number; drawn: unknown; failed: { why: string; at: number } | null } | null {
  const d = drawers.get(cam);
  return d ? { shown: d.shown, paints: d.paints, drawn: d.drawn, failed: d.failed } : null;
}

/** The camera suite's seam: `cam`'s picture throws as it paints from now (`on`), as a browser without a canvas feature would. */
export function failPicture(cam: CameraStore, on: boolean) {
  const d = drawers.get(cam);
  if (d) d.failing = on;
}

/** How a view tells its picture that what it draws changed. */
export interface Drawn {
  /** Paint it again while it shows (the playhead moved), its scene read again first with `read` (the song changed): in the camera's next frame, once. */
  redraw(read?: boolean): void;
}

/**
 * The view of `ref` draws its zoom by the hand (VIEW-5) into the element `el`, over the view: `scene`
 * reads what it draws, `paint` draws that at the camera shown. The latest of each is used. The
 * element starts hidden; the camera shows it while such a zoom lasts.
 */
export function useDrawnZoom<S>(ref: CameraRef, el: RefObject<HTMLElement | null>, scene: () => S, paint: (scene: S, s: CameraState) => unknown): Drawn {
  const cam = useCameraOf(ref);
  const latest = useRef({ scene, paint });
  latest.current = { scene, paint };
  useLayoutEffect(() => {
    const holder = el.current;
    if (!cam || !holder) return;
    holder.style.visibility = "hidden";
    const d: Drawer = {
      picture: { el: holder, scene: () => latest.current.scene(), paint: (sc, s) => latest.current.paint(sc as S, s) },
      scene: null,
      shown: false,
      drawn: null,
      paints: 0,
      failed: null,
      failing: false,
      dirty: false,
      reread: false,
    };
    drawers.set(cam, d);
    return () => {
      if (drawers.get(cam) !== d) return;
      hidePicture(cam);
      drawers.delete(cam);
    };
  }, [cam, el]);
  return useMemo(
    () => ({
      redraw(read = false) {
        const d = cam ? drawers.get(cam) : undefined;
        if (!cam || !d?.shown) return;
        d.dirty = true;
        if (read) d.reread = true;
        wakers.get(cam)?.();
      },
    }),
    [cam],
  );
}
