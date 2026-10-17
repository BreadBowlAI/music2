// Hiding one automation lane (the polish plan's D4, Q15): a lane's header menu has "Hide this
// lane", and a line under the track's lanes says how many are hidden, with Show. A (show
// automation) never shows a hidden lane; Automate on its control, or "Show … lane", does. Kept per
// song in `.music2/view.json` as `hidden: ["bass.cutoff"]`, through the view store (lib/local).

import { LANE_MADE } from "./events";
import { getLocal, setLocal, useLocal } from "./local";

const KEY = "hidden";
const NONE: string[] = [];
/** The view.json name of a lane: `bass.cutoff`. */
const name = (module: string, lane: string) => `${module}.${lane}`;

/** Every hidden lane, as view.json names them: the same list until one is hidden or shown. */
export const useAllHidden = () => useLocal<string[]>(KEY, NONE);

/** The keys of `module`'s lanes that are hidden (some may name lanes that are gone). */
export function useHidden(module: string): ReadonlySet<string> {
  const all = useLocal<string[]>(KEY, NONE);
  const prefix = `${module}.`;
  return new Set(all.filter((a) => a.startsWith(prefix)).map((a) => a.slice(prefix.length)));
}

/** Hide lane `lane` of `module`. */
export function hide(module: string, lane: string) {
  const all = getLocal<string[]>(KEY, NONE);
  if (!all.includes(name(module, lane))) setLocal(KEY, [...all, name(module, lane)]);
}

/** Show `module`'s hidden lanes: those in `lanes`, or every one. */
export function show(module: string, lanes?: readonly string[]) {
  const all = getLocal<string[]>(KEY, NONE);
  const kept = all.filter((a) => !(a.startsWith(`${module}.`) && (!lanes || lanes.includes(a.slice(module.length + 1)))));
  if (kept.length !== all.length) setLocal(KEY, kept.length ? kept : null);
}

// A lane made or asked for (Automate on its control, Show … lane) shows, hidden or not.
window.addEventListener(LANE_MADE, (e) => {
  const id = (e as CustomEvent<string>).detail;
  const dot = id.indexOf(".");
  if (dot > 0) show(id.slice(0, dot), [id.slice(dot + 1)]);
});
