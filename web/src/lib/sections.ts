// Sections (gap #23, ARR-1 to ARR-9): whole bars of every track at once. A drag along the ruler's
// top band, where the markers are, selects them; ⌘C, ⌘X, ⌘V, ⌘D, ⌫, ⌘I, ⇧⌘⌫ and the lanes' menu act
// on them, each one gesture of the server's `POST /api/graph/sections`, one undo step.
//
// One selection at a time (ARR-9): selecting bars lets go of the selected track, clips and
// automation point, and selecting any of those lets go of the bars.
//
// The copy is the server's (`POST /api/graph/sections/copy`): every lane's kind and the clips that
// overlap the bars, whole, lanes of names too. It is kept in this window, as the clips' copy is, and
// the server cuts it to the bars when it is pasted (ARR-5), so a cut and then a paste puts back what
// was cut.

import { create } from "zustand";
import { barAt, readJson, type GraphState, type Meter } from "./graph";
import { send, useClipboard, useClipSelection, type Done, type LaneAddr } from "./clips";
import type { Editing, Step } from "./history";
import { token } from "./native";
import { usePoints } from "./points";
import { useShell } from "./shell";
import { http } from "./http";

/** A copied section (ARR-5): its bars, `[start, end)` in beats, and every lane's kind and the clips that overlap them. */
export interface Section {
  start: number;
  end: number;
  lanes: (LaneAddr & { kind: string; unit?: string; options?: string[]; clips: Record<string, unknown>[] })[];
}

/** What can be done to a section (the server's section `Gesture`, tagged by `op`). Beats on bar lines. */
export type SectionGesture =
  | { op: "paste"; section: Section; at: number }
  | { op: "duplicate"; start: number; end: number; to?: number }
  | { op: "move"; start: number; end: number; to: number }
  | { op: "delete"; start: number; end: number }
  | { op: "insert-bars"; at: number; bars: number }
  | { op: "delete-bars"; start: number; end: number };

/**
 * The selected bars, `[start, end)` in beats on bar lines (null: none), and the copy ⌘C or ⌘X took
 * (null: none). Selecting bars lets go of the selected clips, and picking a clip lets go of the bars;
 * a copy of either is the one ⌘V pastes.
 */
export const useSection = create<{ bars: [number, number] | null; copied: Section | null }>(() => ({ bars: null, copied: null }));

/** Select bars `start` to `end`, across every track: the one selection (ARR-9). */
export function selectBars(start: number, end: number) {
  useClipSelection.getState().clear();
  usePoints.setState({ picked: null });
  if (useShell.getState().selected !== null) useShell.getState().set({ selected: null, auto: false });
  useSection.setState({ bars: [start, end] });
}

/** Let go of the selected bars. */
export const clearBars = () => useSection.getState().bars && useSection.setState({ bars: null });

// Picking clips, an automation point or a track lets go of the bars; copying clips puts them on the
// clipboard instead of a section.
useClipSelection.subscribe((s, was) => s.picked.length > 0 && s.picked !== was.picked && clearBars());
usePoints.subscribe((s, was) => s.picked !== null && s.picked !== was.picked && clearBars());
useShell.subscribe((s, was) => s.selected !== null && s.selected !== was.selected && clearBars());
useClipboard.subscribe((s, was) => s.copied.length > 0 && s.copied !== was.copied && useSection.setState({ copied: null }));

/** Whether `beat` is on a bar line. */
export const onBarLine = (meter: readonly Meter[], beat: number) => Math.abs(barAt(meter, beat).start - beat) < 1e-6;

/** The bar line nearest `beat`. */
export function nearestBarLine(meter: readonly Meter[], beat: number) {
  const b = barAt(meter, beat);
  return beat - b.start < b.start + b.length - beat ? b.start : b.start + b.length;
}

/** How many bars `[start, end)` holds, both on bar lines. */
export function barCount(meter: readonly Meter[], start: number, end: number) {
  return barAt(meter, end - 1e-6).bar - barAt(meter, start).bar + 1;
}

/** `bars 9–16`, or `bar 9`. */
export function barsWords(meter: readonly Meter[], start: number, end: number) {
  const first = barAt(meter, start).bar + 1;
  const last = barAt(meter, Math.max(start, end - 1e-6)).bar + 1;
  return first === last ? `bar ${first}` : `bars ${first}–${last}`;
}

/** The whole bars a drag from beat `lo` to `hi` covers: each end at the bar line nearest it, or, when both meet, the bar `lo` is in. */
export function snapBars(meter: readonly Meter[], lo: number, hi: number) {
  const [start, end] = [nearestBarLine(meter, lo), nearestBarLine(meter, hi)];
  if (end > start) return { start, end };
  const b = barAt(meter, lo);
  return { start: b.start, end: b.start + b.length };
}

/** A copy of bars `start` to `end` of the open song, made by the server (ARR-5). Refused, it throws its words. */
export async function copyBars(graph: GraphState, start: number, end: number): Promise<Section> {
  const res = await http("/api/graph/sections/copy", {
    method: "POST",
    headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
    body: JSON.stringify({ session: graph.session, start, end }),
  });
  const json = await readJson(res);
  if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
  return json.section as Section;
}

/**
 * Send one section gesture: one Tx, one undo step named `step`. Refused, it throws the server's
 * words. `edit`: the gesture's own edit on its way, which its caller ends.
 */
export function sendSection(graph: GraphState, gesture: SectionGesture, step: Step, edit?: Editing, from?: number): Promise<Done> {
  // Through the clips' `send`, whose answer this route shares: an edit on its way, as theirs are.
  // `from`: the revision its bars were drawn at, which the server refuses it over a change since.
  const body = { session: graph.session, gesture, ...(from !== undefined ? { revision: from } : {}) };
  return send(graph, gesture as never, step, { path: "/api/graph/sections", body }, edit);
}
