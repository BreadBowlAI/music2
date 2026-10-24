// An agent's pointer (AGENT-6): `m2 point` shows a small note at a track, a clip, a lane, a knob or
// bars, to teach where things are: "listen to bars 9 to 16", and the musician presses play. One at
// a time; OK dismisses it. It never scrolls, zooms or selects: off screen, it waits at the edge of
// where its target would be, with Show, and only that click moves the view.

import { create } from "zustand";
import { token } from "./native";
import { canvas, useShell } from "./shell";
import { useStore } from "./store";

/** A pointer as the server shows it. */
export interface Pointer {
  session: number;
  target: string;
  kind: "track" | "lane" | "clip" | "knob" | "bars";
  track?: string | null;
  lane?: string;
  clip?: string;
  knob?: string;
  bars?: [number, number];
  beats?: [number, number];
  /** What the agent says there: plain text (AGENT-10). */
  words: string;
  /** The agent's name; null when the page posted it. */
  by?: string | null;
}

declare module "./graph" {
  interface GraphState {
    /** The pointer shown, for a window that loads after it was posted. */
    point?: Pointer | null;
  }
}

export const usePointer = create<{ pointer: Pointer | null }>(() => ({ pointer: null }));

/** The server showed a pointer, or cleared it. */
export const heardPoint = (e: { point: Pointer | null }) => usePointer.setState({ pointer: e.point });

/** The pointer the open song has, when the window connects or another song opens. */
export const syncPointer = () => usePointer.setState({ pointer: useStore.getState().graph?.point ?? null });

/** OK: the pointer goes, for every window and agent. */
export function dismissPointer() {
  const p = usePointer.getState().pointer;
  if (!p) return;
  usePointer.setState({ pointer: null });
  void fetch("/api/point", {
    method: "POST",
    headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
    body: JSON.stringify({ session: p.session, clear: true }),
  }).catch(() => {});
}

/** The rows and things a pointer names, as the page draws them. */
const rowOf = (track: string, lane: string) => document.querySelector<HTMLElement>(`[data-lane="${CSS.escape(`${track}.${lane}`)}"]`);
const headerOf = (track: string) => document.querySelector<HTMLElement>(`[data-module-tab="${CSS.escape(track)}"]`);
const knobOf = (id: string) => document.querySelector<HTMLElement>(`[data-knob-id="${CSS.escape(id)}"]`);

/** Where a pointer's target is drawn, on screen; null when the page doesn't draw it now. */
export function targetBox(p: Pointer): DOMRect | null {
  switch (p.kind) {
    case "track":
      return p.track ? (headerOf(p.track)?.getBoundingClientRect() ?? null) : null;
    case "lane": {
      const row = p.track && p.lane ? rowOf(p.track, p.lane) : null;
      return row ? (row.firstElementChild ?? row).getBoundingClientRect() : null;
    }
    case "clip": {
      const row = p.track && p.lane ? rowOf(p.track, p.lane) : null;
      return row?.querySelector<HTMLElement>(`[data-clip="${CSS.escape(p.clip ?? "")}"]`)?.getBoundingClientRect() ?? null;
    }
    case "knob":
      return p.knob ? (knobOf(p.knob)?.getBoundingClientRect() ?? null) : null;
    case "bars": {
      const ruler = document.querySelector<HTMLElement>("[data-testid=ruler]")?.getBoundingClientRect();
      const [a, b] = [canvas.beatX?.(p.beats?.[0] ?? 0), canvas.beatX?.(p.beats?.[1] ?? 0)];
      return ruler && a != null && b != null ? new DOMRect(a, ruler.top, Math.max(2, b - a), ruler.height) : null;
    }
  }
}

/** Where a pointer's target can be seen: the timeline's lanes, its headers' column, or the dock. */
export function seenIn(p: Pointer): DOMRect | null {
  if (p.kind === "knob") return document.querySelector<HTMLElement>("#dock")?.getBoundingClientRect() ?? null;
  const lanes = canvas.lanesBox?.();
  if (!lanes) return null;
  if (p.kind === "clip") return lanes;
  const timeline = document.querySelector<HTMLElement>("[data-testid=timeline]")?.getBoundingClientRect();
  if (!timeline) return lanes;
  // Bars are on the ruler, above the lanes; tracks and lanes by their headers, left of them.
  if (p.kind === "bars") return new DOMRect(lanes.left, timeline.top, lanes.width, lanes.bottom - timeline.top);
  return new DOMRect(timeline.left, lanes.top, timeline.width, lanes.height);
}

/**
 * Show: the one way a pointer moves the view. The bars scroll into the timeline; a track, a lane or
 * a clip into view, a track's folded automation unfolded for a lane; a knob's track shown in Device.
 */
export function reveal(p: Pointer) {
  const shell = useShell.getState();
  const into = (find: () => HTMLElement | null | undefined, inline: ScrollLogicalPosition = "nearest") =>
    requestAnimationFrame(() => find()?.scrollIntoView({ block: "nearest", inline }));
  switch (p.kind) {
    case "bars":
      if (p.beats) canvas.reveal?.(p.beats[0], p.beats[1]);
      return;
    case "knob":
      if (p.knob && !knobOf(p.knob)) {
        if (p.track) shell.select(p.track);
        if (shell.view !== "device" && shell.view !== "mix") shell.setView("device");
        else if (!shell.dock) shell.setDock(true);
      }
      return into(() => (p.knob ? knobOf(p.knob) : null));
    case "lane":
    case "clip":
      if (p.track && p.lane && !rowOf(p.track, p.lane)) shell.unfold(p.track);
      return into(() => {
        const row = p.track && p.lane ? rowOf(p.track, p.lane) : null;
        // A lane by its header: its row is as wide as the song.
        return p.kind === "clip" ? row?.querySelector<HTMLElement>(`[data-clip="${CSS.escape(p.clip ?? "")}"]`) : (row?.firstElementChild as HTMLElement | null);
      }, p.kind === "clip" ? "center" : "nearest");
    case "track":
      return into(() => (p.track ? headerOf(p.track) : null));
  }
}
