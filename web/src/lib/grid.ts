// The grid and snap of each view that has one: the timeline's (`song`) and the roll's (`notes`),
// drawn by one control (`GridChip`). Each is kept in this browser's storage, as the dock's view
// is. The app's page gets a new address at each launch, so there they start over each time.

import { create } from "zustand";

/** The views with a grid. */
export type GridView = "song" | "notes";

/** The grid's steps, in beats; snap (the magnet) puts what moves on it, or lets it go anywhere. */
export const GRIDS = [
  { label: "1/4", step: 1 },
  { label: "1/8", step: 0.5 },
  { label: "1/16", step: 0.25 },
  { label: "1/32", step: 0.125 },
];

/** A view's grid step in beats, and whether snap is on. */
export interface Grid {
  step: number;
  on: boolean;
}

/** Each view's grid until one is chosen: the timeline's beats, the roll's sixteenths. */
const START: Record<GridView, number> = { song: 1, notes: 0.25 };

/**
 * Where each view's choice is kept: `snap` is the grid, or 0 when snap is off; `grid` keeps the
 * grid apart, so that turning snap off and on keeps it. The roll's keys are the ones it had.
 */
const KEYS: Record<GridView, { snap: string; grid: string }> = {
  song: { snap: "m2.song.snap", grid: "m2.song.grid" },
  notes: { snap: "m2.notes.snap", grid: "m2.notes.grid" },
};

function read(view: GridView): Grid {
  const load = (key: string) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  };
  const valid = (x: string | null) => x !== null && GRIDS.some((g) => g.step === Number(x));
  const snap = load(KEYS[view].snap), grid = load(KEYS[view].grid);
  return { step: valid(grid) ? Number(grid) : valid(snap) ? Number(snap) : START[view], on: snap !== "0" };
}

interface Grids extends Record<GridView, Grid> {
  /** The view ⌘1 to ⌘4 act on: the one the pointer or the keyboard was last in. */
  last: GridView;
  set: (view: GridView, grid: Grid) => void;
}

export const useGrid = create<Grids>((set) => ({
  song: read("song"),
  notes: read("notes"),
  last: "song",
  set: (view, grid) => {
    try {
      localStorage.setItem(KEYS[view].grid, String(grid.step));
      localStorage.setItem(KEYS[view].snap, String(grid.on ? grid.step : 0));
    } catch {}
    set({ [view]: grid } as Pick<Grids, GridView>);
  },
}));

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
