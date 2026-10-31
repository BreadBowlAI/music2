// The window's shell: what the dock shows, whether the dock and the browser are open, what is
// selected, which tracks show their automation. One store, so the keyboard, the menu bar and the
// views all change the same state (taste.md §3.5–§3.7).
//
// The dock's view, its height and the browser are per window, kept in this browser's storage
// (view.json, per song, comes later). The selection and the folds belong to the open song: a new
// session starts them over (`resetSelection`).

import { create } from "zustand";
import { DOCK_H, DOCK_MIN, WIDE, dockMax } from "./layout";
import type { ClipRef } from "./graph";

/** The dock's views, in the order of the switcher and of ⌥1 to ⌥4. */
export const VIEWS = ["device", "notes", "mix", "code"] as const;
export type DockView = (typeof VIEWS)[number];

const VIEW_KEY = "m2.dock.view";
const OPEN_KEY = "m2.dock.open";
/** Kept under the name the graph panel had, so a height set before stays. */
const HEIGHT_KEY = "m2.graph.panel";
const BROWSER_KEY = "m2.browser.open";

const load = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const save = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {}
};

export const clampDock = (h: number) => Math.round(Math.min(Math.max(h, DOCK_MIN), dockMax(window.innerHeight)));

interface Shell {
  /** The dock's view: one choice for the window, changed only by the producer, never by a selection (§3.7). */
  view: DockView;
  /** The dock's body shows (E hides it; its header stays). */
  dock: boolean;
  /** The dock body's height, as set by its splitter. */
  height: number;
  browser: boolean;
  /** The selected track (a module key), or null after Esc: the dock then keeps showing `module`. */
  selected: string | null;
  /** What the dock shows: the selected track, else the last one selected. */
  module: string | null;
  /** Nothing chosen yet in this song: the first track is selected for the producer, and follows when the first track appears. */
  auto: boolean;
  /** Each track's selected notes clip: Notes shows the selected track's. */
  clips: Record<string, ClipRef>;
  /** Tracks whose automation lanes show (folded by default). */
  open: ReadonlySet<string>;
  /** The keyboard shortcuts sheet (⌥K). */
  sheet: boolean;
  /** A rename asked for (⌘R, a menu's Rename): the track whose name turns into a field, in its header or its Mix strip. */
  renaming: { module: string; at: "header" | "strip" } | null;
  /** The roll's Draw mode (B): a click adds a note and a drag paints; else a click selects (D4). */
  draw: boolean;
  /**
   * The chord the roll stamps (NOTE-4): a kind of the server's table ("triad", "seventh"…) and its
   * inversion; null for single notes. The window's, as Draw mode is.
   */
  chord: { kind: string; inversion: number } | null;
  /** A place the Code view opens at, once it can: a file and a line (a Mix strip's Edit in code). */
  codeAt: { file: string; line: number } | null;

  setView: (view: DockView) => void;
  setDock: (open: boolean) => void;
  setHeight: (h: number, keep?: boolean) => void;
  setBrowser: (open: boolean) => void;
  /** Select a track (and with `clip`, one of its notes clips). */
  select: (module: string, clip?: ClipRef | null) => void;
  /** Esc: the clip first, then the track. */
  deselect: () => void;
  setClip: (module: string, clip: ClipRef | null) => void;
  /** Fold or unfold one track's automation, or with no track, every track's. */
  toggleLanes: (module: string | null, all: string[]) => void;
  unfold: (module: string) => void;
  set: (p: Partial<Shell>) => void;
  /** A new song: nothing selected, everything folded. */
  resetSelection: () => void;
}

const startView = (): DockView => {
  const v = load(VIEW_KEY);
  return (VIEWS as readonly string[]).includes(v ?? "") ? (v as DockView) : "device";
};

export const useShell = create<Shell>((set, get) => ({
  view: startView(),
  dock: load(OPEN_KEY) !== "false",
  height: (() => {
    const saved = Number(load(HEIGHT_KEY));
    return clampDock(saved > 0 ? saved : DOCK_H);
  })(),
  // Shown in a wide window, hidden in a small one, unless chosen before (§3.2).
  browser: (() => {
    const saved = load(BROWSER_KEY);
    return saved === null ? window.innerWidth >= WIDE : saved === "true";
  })(),
  selected: null,
  module: null,
  auto: true,
  clips: {},
  open: new Set(),
  sheet: false,
  renaming: null,
  draw: false,
  chord: null,
  codeAt: null,

  setView: (view) => {
    save(VIEW_KEY, view);
    // Choosing a view shows the dock.
    if (!get().dock) save(OPEN_KEY, "true");
    set({ view, dock: true });
  },
  setDock: (dock) => {
    save(OPEN_KEY, String(dock));
    set({ dock });
  },
  setHeight: (h, keep = false) => {
    const height = clampDock(h);
    if (keep) save(HEIGHT_KEY, String(height));
    set({ height });
  },
  setBrowser: (browser) => {
    save(BROWSER_KEY, String(browser));
    set({ browser });
  },
  select: (module, clip) =>
    set((s) => {
      const clips = { ...s.clips };
      if (clip) clips[module] = clip;
      else if (clip === null) delete clips[module];
      return { selected: module, module, clips, auto: false };
    }),
  deselect: () =>
    set((s) => {
      const m = s.selected;
      if (m && s.clips[m]) {
        const clips = { ...s.clips };
        delete clips[m];
        return { clips };
      }
      return { selected: null, auto: false };
    }),
  setClip: (module, clip) =>
    set((s) => {
      const clips = { ...s.clips };
      if (clip) clips[module] = clip;
      else delete clips[module];
      return { clips };
    }),
  toggleLanes: (module, all) =>
    set((s) => {
      const open = new Set(s.open);
      if (module) {
        if (open.has(module)) open.delete(module);
        else open.add(module);
        return { open };
      }
      // Every track: unfold them all, unless all are open already.
      const every = all.length > 0 && all.every((m) => open.has(m));
      return { open: every ? new Set() : new Set(all) };
    }),
  unfold: (module) => set((s) => (s.open.has(module) ? {} : { open: new Set([...s.open, module]) })),
  set: (p) => set(p),
  resetSelection: () => set({ selected: null, module: null, auto: true, clips: {}, open: new Set(), renaming: null }),
}));

/** What ⌘1 to ⌘4 (View ▸ the grid rows) do to the grid of the view last used. */
export type GridOp = "narrower" | "wider" | "triplet" | "snap";

/**
 * What the open song's canvas does for a command it alone can: the transport (it knows where
 * playback is and refreshes it), the loop and the song bar's name field. The keys and the menu
 * rows of mute and solo, the grid and the clips come here too; each is filled by the view that
 * does it, and does nothing until then.
 */
export const canvas: {
  loop?: () => void;
  loopSelection?: () => void;
  play?: () => void;
  home?: () => void;
  /** M, Track ▸ Mute (⌃M on a Mac): mute or unmute track `module` while listening. */
  mute?: (module: string) => void;
  /** S, Track ▸ Solo (⌃S on a Mac). */
  solo?: (module: string) => void;
  /** Track ▸ Mute in Song: one undo step. */
  muteInSong?: (module: string) => void;
  /** Track ▸ Change Instrument…: the instrument list for track `module` (polish A3). */
  changeInstrument?: (module: string) => void;
  grid?: (op: GridOp) => void;
  /** ⌘D on the selected clips, after the roll's notes: false when none is selected. */
  duplicate?: () => boolean;
  /** ⌘E, ⌘J, Edit ▸ Quantize: on the selected clips. */
  split?: () => void;
  join?: () => void;
  quantize?: () => void;
  /** Edit ▸ Insert Bars (⌘I) and Delete Bars (⇧⌘⌫): on the selected bars (ARR-9). */
  insertBars?: () => void;
  deleteBars?: () => void;
  /** Whether ⌘C, ⌘X and ⌘V are the timeline's now (clips are selected), not the page's. */
  takesClipboard?: () => boolean;
  clipboard?: (op: "copy" | "cut" | "paste") => void;
  /** View ▸ Zoom In (1) and Zoom Out (-1): the timeline's, around the playhead or the view's middle. */
  zoom?: (dir: 1 | -1) => void;
  /** Z, View ▸ Zoom to Fit: the selected clips, or the whole song; again, back. */
  zoomFit?: () => void;
  /** Transport ▸ Follow Playhead: on or off. */
  follow?: () => void;
  /** View ▸ Taller (1) and Shorter (-1) Tracks: the selected track, or every track. */
  height?: (dir: 1 | -1) => void;
  /** Transport ▸ Add Marker (at the playhead's bar), Previous and Next Marker. */
  marker?: (op: "add" | "prev" | "next") => void;
  /** Where beat `b` is on screen, as a client x (an agent's pointer at bars, AGENT-6). */
  beatX?: (b: number) => number | null;
  /** The timeline's lanes on screen: right of the headers, under the ruler, above the Song row. */
  lanesBox?: () => DOMRect | null;
  /** A pointer's Show: beats `start` to `end` in view. */
  reveal?: (start: number, end: number) => void;
} = {};
