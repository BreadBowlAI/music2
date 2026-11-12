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

/** Who hears a press or a key: each camera stops its glide and drops wheel events until a gap (VIEW-4). */
const hearing = new Set<() => void>();
let held = false;

/** Whether a pointer is pressed: a drag holds the camera still (VIEW-8). */
export const pointerHeld = () => held;

function press() {
  held = true;
  for (const f of hearing) f();
}
function release() {
  held = false;
}
function key(e: KeyboardEvent) {
  // ⌘ for ⌘-scroll, ⇧ for a sideways wheel: a modifier alone is no key that stops anything.
  if (e.key === "Meta" || e.key === "Control" || e.key === "Shift" || e.key === "Alt") return;
  for (const f of hearing) f();
}

let listening = false;
/** Hear presses and keys while any camera does. */
export function hearInput(f: () => void): () => void {
  hearing.add(f);
  if (!listening) {
    listening = true;
    window.addEventListener("pointerdown", press, true);
    window.addEventListener("pointerup", release, true);
    window.addEventListener("pointercancel", release, true);
    window.addEventListener("blur", release);
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
    window.removeEventListener("blur", release);
    window.removeEventListener("keydown", key, true);
  };
}

// ---------------------------------------------------------------- moving

let movedAt = -Infinity;

/** A camera moved by the hand, a glide or a zoom, now. */
export const moved = () => void (movedAt = performance.now());

/** Whether a camera moved in the last `QUIET` ms: the views' hover does nothing then (VIEW-8). Read in a handler: it renders nothing. */
export const movingNow = () => performance.now() - movedAt < QUIET;
