// What every camera shares: the platform the app says it runs on, whether the swipe probe has the
// swipes, whether a pointer drag holds the pointer, and whether a camera moved just now.

import { QUIET, SYSTEM_GLIDE } from "./numbers";

/** What the app tells the library of where it runs: it never reads `navigator` itself. */
export interface Platform {
  /** A Mac: the system glides after a flick, and the page hears it as wheel events. */
  mac: boolean;
  /** The musician asked for reduced motion: the camera never glides on its own. */
  still?: boolean;
}

let platform: Platform = { mac: false };
/** The `camera` suite's setting of the glide, over the platform's: null leaves it to the platform. */
let glideSet: boolean | null = null;

/** Say where the app runs (VIEW-4). */
export function setPlatform(p: Platform) {
  platform = { ...p };
}

/** Whether the camera glides on its own after a swipe: where the platform gives the page no glide, unless motion is reduced. */
export const ownGlide = () => glideSet ?? (!(platform.mac ? SYSTEM_GLIDE.mac : SYSTEM_GLIDE.other) && !platform.still);

/** The suite's flag: glide on or off whatever the platform, or (null) as the platform says. */
export const setGlide = (on: boolean | null) => void (glideSet = on);

let paused = 0;
/**
 * Leave plain swipes to the web view until the function this returns is called (the swipe probe,
 * VIEW-10): the camera takes the box's moves in, and still zooms.
 */
export function pauseSwipes(): () => void {
  paused++;
  let done = false;
  return () => {
    if (!done) paused--;
    done = true;
  };
}
export const swipesPaused = () => paused > 0;

// ---------------------------------------------------------------- the pointer and the keys

/**
 * Who hears a press or a key: each camera stops its glide and drops wheel events until a gap (VIEW-4).
 * Null: the window lost focus or was hidden, where a pinch's `gestureend` may never come.
 */
const hearing = new Set<(e: PointerEvent | KeyboardEvent | null) => void>();
let held = false;

/** Whether a pointer is pressed: a drag holds the camera still (VIEW-8). */
export const pointerHeld = () => held;

/**
 * Any press stops a glide and drops the swipe still arriving; only the primary button's holds. A
 * press whose release the page may never hear (a context menu opened on it, the window hidden, the
 * pointer moving with no button down) holds no longer.
 */
function press(e: PointerEvent) {
  if (e.button === 0) held = true;
  for (const f of hearing) f(e);
}
function release() {
  held = false;
}
let pointerAt = -Infinity;
let last = { x: NaN, y: NaN };
function hover(e: PointerEvent) {
  if (e.buttons === 0) held = false;
  // A move of the pointer itself, not the content moving under it.
  if (e.clientX === last.x && e.clientY === last.y) return;
  last = { x: e.clientX, y: e.clientY };
  pointerAt = performance.now();
  // Hover again, once no camera moves (VIEW-8).
  if (unhovered && !movingNow()) hoverAgain();
}
/** The window lost focus or was hidden: nothing held, and each camera lays a drawn zoom out (VIEW-5). */
function leave() {
  release();
  for (const f of hearing) f(null);
}
const hidden = () => document.visibilityState === "hidden" && leave();
function key(e: KeyboardEvent) {
  // ⌘ for ⌘-scroll, ⇧ for a sideways wheel: a modifier alone is no key that stops anything.
  if (e.key === "Meta" || e.key === "Control" || e.key === "Shift" || e.key === "Alt") return;
  for (const f of hearing) f(e);
}

/** What a press brings after it, which a press the camera took takes too: its release, its click, its menu. */
const AFTER = ["pointerup", "mousedown", "mouseup", "click", "auxclick", "contextmenu"] as const;
/** How long after a press the camera took its double-click is taken too, in ms: the second press may come that late. */
const DOUBLE = 500;

/**
 * A press the camera takes (a press on a drawn zoom's picture ends the zoom and does nothing else,
 * VIEW-5): nothing else hears it, nor its release, click or menu (until the next press), nor a
 * double-click it begins. Called while it is heard, at the window, before anything below.
 */
export function takePress(e: PointerEvent) {
  e.preventDefault();
  e.stopImmediatePropagation();
  const eat = (ev: Event) => {
    if (ev instanceof PointerEvent && ev.pointerId !== e.pointerId) return;
    ev.preventDefault();
    ev.stopImmediatePropagation();
    // A click ends what a press brings.
    if (ev.type === "click" || ev.type === "auxclick") done();
  };
  const until = e.timeStamp + DOUBLE;
  const double = (ev: Event) => {
    window.removeEventListener("dblclick", double, true);
    if (ev.timeStamp <= until) eat(ev);
  };
  const done = () => {
    window.removeEventListener("pointerdown", next, true);
    for (const t of AFTER) window.removeEventListener(t, eat, true);
  };
  const next = (ev: PointerEvent) => void (ev !== e && done());
  for (const t of AFTER) window.addEventListener(t, eat, true);
  window.addEventListener("pointerdown", next, true);
  window.addEventListener("dblclick", double, true);
  setTimeout(() => window.removeEventListener("dblclick", double, true), DOUBLE);
}

let listening = false;
/** Hear presses and keys while any camera does. */
export function hearInput(f: (e: PointerEvent | KeyboardEvent | null) => void): () => void {
  hearing.add(f);
  if (!listening) {
    listening = true;
    window.addEventListener("pointerdown", press, true);
    window.addEventListener("pointerup", release, true);
    window.addEventListener("pointercancel", release, true);
    window.addEventListener("lostpointercapture", release, true);
    window.addEventListener("contextmenu", release, true);
    window.addEventListener("pointermove", hover, true);
    window.addEventListener("blur", leave);
    document.addEventListener("visibilitychange", hidden);
    window.addEventListener("keydown", key, true);
  }
  return () => {
    hearing.delete(f);
    if (hearing.size || !listening) return;
    listening = false;
    held = false;
    window.removeEventListener("pointerdown", press, true);
    window.removeEventListener("pointerup", release, true);
    window.removeEventListener("pointercancel", release, true);
    window.removeEventListener("lostpointercapture", release, true);
    window.removeEventListener("contextmenu", release, true);
    window.removeEventListener("pointermove", hover, true);
    window.removeEventListener("blur", leave);
    document.removeEventListener("visibilitychange", hidden);
    window.removeEventListener("keydown", key, true);
    hoverAgain();
  };
}

// ---------------------------------------------------------------- moving

let movedAt = -Infinity;
/**
 * Whether the root has `data-camera-moved`: a camera moved by the hand, a glide or a zoom, and the
 * pointer hasn't moved itself since. The app's hover rules keep to it (`hover:` in `index.css`): no
 * CSS hover while a view moves, nor after it until the pointer moves, so what the still pointer is
 * over never changes as a zoom ends or a swipe stops (VIEW-8, VIEW-13).
 */
let unhovered = false;
const MOVED = "data-camera-moved";
function hoverAgain() {
  if (!unhovered) return;
  unhovered = false;
  document.documentElement.removeAttribute(MOVED);
}

/** A camera moved by the hand, a glide or a zoom, now: no hover until the pointer itself moves after it. */
export const moved = () => {
  movedAt = performance.now();
  if (unhovered) return;
  unhovered = true;
  document.documentElement.setAttribute(MOVED, "");
};

/** Whether a camera moved in the last `QUIET` ms: the views' hover does nothing then (VIEW-8). Read in a handler: it renders nothing. */
export const movingNow = () => performance.now() - movedAt < QUIET;

/**
 * Whether a control under the pointer may take a new swipe (VIEW-2): no camera moved in the last
 * `QUIET` ms, or the pointer itself moved since one did. Not a control a camera just carried under a
 * still pointer.
 */
export const controlMayTake = () => !movingNow() || pointerAt > movedAt;
