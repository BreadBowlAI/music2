// The grid and snap of each view that has one: the timeline's (`song`) and the roll's (`notes`),
// drawn by one control (`GridChip`). Each is kept in this browser's storage, as the dock's view
// is. The app's page gets a new address at each launch, so there they start over each time.

import { create } from "zustand";
import { barAt, type Meter } from "./graph";
import type { GridOp } from "./shell";

/** The views with a grid. */
export type GridView = "song" | "notes";

/**
 * A grid's choice (design-8b §7.3): a step in beats (a beat is a quarter note), the bar, or Auto:
 * the finest of bar, beat, 1/8, 1/16 and 1/32 whose lines are at least 12 px apart.
 */
export type GridPick = number | "bar" | "auto";

/** Each view's choices, coarse to fine after Auto; the roll's run 1/4 to 1/64 (taste.md §3.5). */
export const PICKS: Record<GridView, GridPick[]> = {
  song: ["auto", "bar", 2, 1, 0.5, 0.25, 0.125, 0.0625],
  notes: [1, 0.5, 0.25, 0.125, 0.0625],
};

/** The closest grid lines Auto picks, in px. */
const AUTO_PX = 12;
/** What Auto picks from, fine to coarse (`bar` stands for the bar's length there). */
const AUTO: (number | "bar")[] = [0.125, 0.25, 0.5, 1, "bar"];

/** A view's grid: its choice, whether it is in triplets (each step ⅔ as wide), whether snap is on. */
export interface Grid {
  pick: GridPick;
  triplet: boolean;
  on: boolean;
  /**
   * The step in beats, for a fixed choice. The roll's choices are all fixed. The timeline's Auto
   * and Bar depend on its zoom and the bar, so it asks `stepOf` instead.
   */
  step: number;
}

/** A choice's name: "Auto", "Bar", "1/16"; in triplets "1/16T", "Auto T". */
export function pickName(pick: GridPick, triplet = false) {
  const base = pick === "auto" ? "Auto" : pick === "bar" ? "Bar" : `1/${Math.round(4 / pick)}`;
  return triplet ? (typeof pick === "number" ? `${base}T` : `${base} T`) : base;
}

/** The step of `pick` in beats, in a bar of `bar` beats at `ppb` px a beat. */
function stepOfPick(pick: GridPick, triplet: boolean, ppb: number, bar: number): number {
  const t = triplet ? 2 / 3 : 1;
  if (pick === "bar") return bar * t;
  if (typeof pick === "number") return pick * t;
  // Auto: the finest whose lines are far enough apart, else the bar.
  return stepOfPick(AUTO.find((p) => (p === "bar" ? bar : p) * t * ppb >= AUTO_PX - 1e-9) ?? "bar", triplet, ppb, bar);
}

/** A grid's step in beats, in a bar of `bar` beats (4 in 4/4) at `ppb` px a beat. */
export const stepOf = (g: Grid, ppb: number, bar = 4) => stepOfPick(g.pick, g.triplet, ppb, bar);

/** What Auto is now at `ppb`, in 4/4: "1/8" at the timeline's 24 px a beat. */
export function autoName(g: Grid, ppb: number) {
  const step = stepOfPick("auto", false, ppb, 4);
  return pickName(step === 4 ? "bar" : step, g.triplet);
}

/** Beats as the lane format writes them: no float noise past a millionth, as the roll writes notes. */
export const clean = (x: number) => Math.round(x * 1e6) / 1e6 + 0;

/**
 * Where beat `b` lands on a grid of `step` beats: its nearest line. Lines start again at each bar
 * line, as the drawn grid does, so a triplet grid in 3/4 still lands on every bar line.
 */
export function snapTo(meter: readonly Meter[], b: number, step: number) {
  const bar = barAt(meter, Math.max(0, b));
  const into = Math.max(0, b - bar.start);
  const lo = Math.floor(into / step + 1e-9) * step;
  const hi = Math.min(lo + step, bar.length);
  return clean(bar.start + (into - lo <= hi - into ? lo : hi));
}

/**
 * Where beat `b` lands in the timeline at `ppb` px a beat: on its grid while snap is on and the
 * gesture doesn't skip it (`free`: ⌘ held), anywhere otherwise. Never before the song's start.
 */
export function snapSong(meter: readonly Meter[], b: number, ppb: number, free = false) {
  const g = useGrid.getState().song;
  const at = Math.max(0, b);
  return !g.on || free ? clean(at) : snapTo(meter, at, stepOf(g, ppb, barAt(meter, at).length));
}

/** Whether a pointer event skips the snap: ⌘ held on a Mac, Ctrl elsewhere (either works on both). */
export const skipsSnap = (e: { metaKey: boolean; ctrlKey: boolean }) => e.metaKey || e.ctrlKey;

/** Each view's grid until one is chosen: the timeline's Auto, the roll's sixteenths. */
const START: Record<GridView, GridPick> = { song: "auto", notes: 0.25 };

/**
 * Where each view's choice is kept: `grid` is the choice, `snap` is "0" while snap is off, and
 * `triplet` is "1" in triplets. The roll's first two keys are the ones it had.
 */
const KEYS: Record<GridView, { snap: string; grid: string; triplet: string }> = {
  song: { snap: "m2.song.snap", grid: "m2.song.grid", triplet: "m2.song.triplet" },
  notes: { snap: "m2.notes.snap", grid: "m2.notes.grid", triplet: "m2.notes.triplet" },
};

/** A grid with its step worked out (for Auto and Bar, as at 24 px a beat in 4/4: see `step`). */
const grid = (pick: GridPick, triplet: boolean, on: boolean): Grid => ({ pick, triplet, on, step: stepOfPick(pick, triplet, 24, 4) });

function read(view: GridView): Grid {
  const load = (key: string) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  };
  const parse = (x: string | null): GridPick | null => {
    if (x === null) return null;
    const p = x === "auto" || x === "bar" ? x : Number(x);
    return PICKS[view].includes(p) ? p : null;
  };
  const snap = load(KEYS[view].snap);
  return grid(parse(load(KEYS[view].grid)) ?? parse(snap) ?? START[view], load(KEYS[view].triplet) === "1", snap !== "0");
}

interface Grids extends Record<GridView, Grid> {
  /** The view ⌘1 to ⌘4 act on: the one the pointer or the keyboard was last in. */
  last: GridView;
  /** Change a view's grid (its step is worked out again) and keep it. */
  set: (view: GridView, grid: Omit<Grid, "step">) => void;
}

export const useGrid = create<Grids>((set) => ({
  song: read("song"),
  notes: read("notes"),
  last: "song",
  set: (view, g) => {
    try {
      localStorage.setItem(KEYS[view].grid, String(g.pick));
      localStorage.setItem(KEYS[view].snap, g.on ? String(g.pick) : "0");
      localStorage.setItem(KEYS[view].triplet, g.triplet ? "1" : "0");
    } catch {}
    set({ [view]: grid(g.pick, g.triplet, g.on) } as Pick<Grids, GridView>);
  },
}));

/**
 * ⌘1 to ⌘4 on `view`'s grid: a step narrower or wider through its fixed choices (from Auto, the
 * step Auto is at `ppb` now), triplets on or off, snap on or off.
 */
export function gridOp(view: GridView, op: GridOp, ppb: number) {
  const { set, [view]: g } = useGrid.getState();
  if (op === "snap") return set(view, { ...g, on: !g.on });
  if (op === "triplet") return set(view, { ...g, triplet: !g.triplet });
  const fixed = PICKS[view].filter((p) => p !== "auto");
  const size = (p: GridPick) => (p === "bar" ? 4 : p === "auto" ? stepOfPick("auto", false, ppb, 4) : p);
  const now = size(g.pick);
  const pick = op === "narrower" ? fixed.find((p) => size(p) < now - 1e-9) : fixed.findLast((p) => size(p) > now + 1e-9);
  // At either end a fixed choice stays where it is; Auto becomes the fixed step it was.
  set(view, { ...g, pick: pick ?? (g.pick === "auto" ? (now === 4 ? "bar" : now) : g.pick) });
}

/** The attribute that marks a view's root with its grid: `data-grid-view="notes"`. */
export const GRID_VIEW = "data-grid-view";

/**
 * Follow the view the pointer or the keyboard was last in, for ⌘1 to ⌘4 (taste.md §5.1): a press
 * or a focus inside an element marked `data-grid-view`. Returns how to stop.
 */
export function followGridView() {
  const seen = (e: Event) => {
    const at = e.target instanceof Element ? e.target.closest(`[${GRID_VIEW}]`) : null;
    const view = at?.getAttribute(GRID_VIEW);
    if ((view === "song" || view === "notes") && useGrid.getState().last !== view) useGrid.setState({ last: view });
  };
  window.addEventListener("pointerdown", seen, true);
  window.addEventListener("focusin", seen, true);
  return () => {
    window.removeEventListener("pointerdown", seen, true);
    window.removeEventListener("focusin", seen, true);
  };
}
