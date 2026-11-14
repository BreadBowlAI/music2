// The frame meter's numbers (taste.md VIEW-10): frames a second and the slowest frame, from
// `requestAnimationFrame`, and each view's redraws, from one `useRenderCount` line at the top of
// each view and of its rows. Plain module state: nothing here redraws anything, and it works in the
// app's own build, where React's Profiler does nothing (the camera note's C11).

import { useLayoutEffect, useRef } from "react";
import { create } from "zustand";
import { face } from "../camera";

const SECOND = 1000;

/** Each name's redraws since the page loaded. */
const total: Record<string, number> = {};
/** Each name's redraws in about the last second: their times, oldest first. */
const recent: Record<string, number[]> = {};

function count(name: string) {
  const now = performance.now();
  total[name] = (total[name] ?? 0) + 1;
  const times = (recent[name] ??= []);
  times.push(now);
  while (times[0] < now - SECOND) times.shift();
}

/**
 * Count a redraw of `name` (`"arrangement"`, `"arrangement.rows"`…): one line at the top of a view's
 * or a row's component. A redraw is a render React puts on the page (counted as it does, in a layout
 * effect), not a render it starts and drops, as it may a transition's (VIEW-12). `first`, if given,
 * names the component's first redraw apart (a clip entering the window). Only a counter: no state,
 * nothing that itself redraws.
 */
export function useRenderCount(name: string, first?: string) {
  const drawn = useRef(false);
  // This render's own mark: React's StrictMode (in a development build) runs a new component's
  // effects twice for one commit, and that is counted once.
  const render = {};
  const counted = useRef<object | null>(null);
  useLayoutEffect(() => {
    if (counted.current === render) return;
    counted.current = render;
    count(first && !drawn.current ? first : name);
    drawn.current = true;
  });
}

/** Count a redraw that is no React render, `name` as a render's: a clip's canvas drawn again (VIEW-12). */
export const countDraw = (name: string) => count(name);

/** The frames' times while something measures them, from just before the last second on. */
const times: number[] = [];
/** Who hears each frame's interval (the probe). */
const hearing = new Set<(dt: number, at: number) => void>();
let measuring = 0;
let raf = 0;
/** Frames measured since the page loaded: a suite sees the loop stop when nothing measures. */
let measured = 0;

function frame(at: number) {
  const last = times.at(-1);
  times.push(at);
  measured++;
  while (times.length > 2 && times[1] < at - SECOND) times.shift();
  if (last !== undefined) for (const f of hearing) f(at - last, at);
  raf = requestAnimationFrame(frame);
}

/**
 * Measure the frames until the function this returns is called: while the meter shows, and while
 * the probe listens. A frame loop runs only then. `each` hears every frame's interval.
 */
export function measureFrames(each?: (dt: number, at: number) => void): () => void {
  if (each) hearing.add(each);
  if (measuring++ === 0) {
    times.length = 0;
    raf = requestAnimationFrame(frame);
  }
  let done = false;
  return () => {
    if (done) return;
    done = true;
    if (each) hearing.delete(each);
    if (--measuring === 0) {
      cancelAnimationFrame(raf);
      times.length = 0;
    }
  };
}

export interface Frames {
  /** Frames in the last second; null while nothing measures them. */
  fps: number | null;
  /** The slowest of them, in ms (to a tenth); null while nothing measures them. */
  slowest: number | null;
  /** Each name's redraws in the last second. */
  renders: Record<string, number>;
  /** Each name's redraws since the page loaded. */
  total: Record<string, number>;
  /** Frames measured since the page loaded. */
  measured: number;
}

/** The meter's numbers, now. */
export function frames(): Frames {
  const now = performance.now();
  const renders: Record<string, number> = {};
  for (const [name, list] of Object.entries(recent)) {
    while (list[0] < now - SECOND) list.shift();
    renders[name] = list.length;
  }
  if (!measuring) return { fps: null, slowest: null, renders, total: { ...total }, measured };
  let fps = 0, slowest = 0;
  for (let i = 1; i < times.length; i++) {
    if (times[i] < now - SECOND) continue;
    fps++;
    slowest = Math.max(slowest, times[i] - times[i - 1]);
  }
  return { fps, slowest: Math.round(slowest * 10) / 10, renders, total: { ...total }, measured };
}

/** The zoom bench, a file of its own, loaded only when asked for: by a suite here, or by the swipe probe's last step (VIEW-10). */
const bench = () => import("./zoomBench");

declare global {
  interface Window {
    /** What the page gives the browser suites, and only that: the meter's numbers, the camera library's face, and the zoom bench (VIEW-10). */
    m2canvas?: { frames: typeof frames; camera: typeof face; bench: typeof bench };
  }
}
window.m2canvas = { frames, camera: face, bench };

/** Whether the frame meter shows (the command palette's Show frame rate). The window's own, not kept. */
export const useFrameMeter = create<{ on: boolean }>(() => ({ on: false }));
export const toggleFrameMeter = () => useFrameMeter.setState((s) => ({ on: !s.on }));
