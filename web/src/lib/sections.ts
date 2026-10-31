// Sections (gap #23, ARR-1 to ARR-9): whole bars of every track at once. A drag across the ruler
// selects them (and loops them, TL-10); ⌘C, ⌘X, ⌘V, ⌘D, ⌫, ⌘I, ⇧⌘⌫ and the lanes' menu act on them,
// each one gesture of the server's `POST /api/graph/sections`, one undo step.
//
// The copy is kept in this window, as the clips' is: every lane's clips that overlap the bars,
// whole, as the timeline draws them. The server cuts them to the bars when they are pasted (ARR-5),
// so a cut and then a paste puts back what was cut.

import { create } from "zustand";
import { barAt, type GraphState, type Meter, type TimelineView } from "./graph";
import { send, useClipboard, useClipSelection, type Done, type LaneAddr } from "./clips";
import type { Step } from "./history";

/** A copied section (ARR-5): its bars, `[start, end)` in beats, and every lane's clips that overlap them. */
export interface Section {
  start: number;
  end: number;
  lanes: (LaneAddr & { clips: Record<string, unknown>[] })[];
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

/** Select bars `start` to `end`, across every track. */
export function selectBars(start: number, end: number) {
  useClipSelection.getState().clear();
  useSection.setState({ bars: [start, end] });
}

/** Let go of the selected bars. */
export const clearBars = () => useSection.getState().bars && useSection.setState({ bars: null });

// Picking clips lets go of the bars; copying clips puts them on the clipboard instead of a section.
useClipSelection.subscribe((s, was) => s.picked.length > 0 && s.picked !== was.picked && clearBars());
useClipboard.subscribe((s, was) => s.copied.length > 0 && s.copied !== was.copied && useSection.setState({ copied: null }));

/** Whether `beat` is on a bar line. */
export const onBarLine = (meter: readonly Meter[], beat: number) => Math.abs(barAt(meter, beat).start - beat) < 1e-6;

/** The bar line nearest `beat`. */
export function nearestBarLine(meter: readonly Meter[], beat: number) {
  const b = barAt(meter, beat);
  return beat - b.start < b.start + b.length - beat ? b.start : b.start + b.length;
}

/** The bars a drag from beat `lo` to `hi` covers: each end at the nearest bar line, or, both nearest the same one, the bar `lo` is in. */
export function snapBars(meter: readonly Meter[], lo: number, hi: number) {
  const [start, end] = [nearestBarLine(meter, lo), nearestBarLine(meter, hi)];
  if (end > start) return { start, end };
  const b = barAt(meter, lo);
  return { start: b.start, end: b.start + b.length };
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

/** The section `start` to `end` of `timeline`, as the clipboard keeps it: every lane, each with its clips that overlap the bars, whole. */
export function sectionOf(timeline: TimelineView, start: number, end: number): Section {
  const lanes = timeline.tracks.flatMap((t) =>
    t.lanes.map((l) => ({
      module: t.module,
      lane: l.key,
      clips: (l.clips as { start: number; length: number }[])
        .filter((c) => c.start < end && c.start + c.length > start)
        .map((c) => {
          // Without the hash the timeline drew it with: a copy names no clip of the song.
          const { sha: _sha, ...rest } = c as unknown as Record<string, unknown>;
          return rest;
        }),
    })),
  );
  return { start, end, lanes };
}

/** Send one section gesture: one Tx, one undo step named `step`. Refused, it throws the server's words. */
export function sendSection(graph: GraphState, gesture: SectionGesture, step: Step): Promise<Done> {
  // Through the clips' `send`, whose answer this route shares: an edit on its way, as theirs are.
  return send(graph, gesture as never, step, { path: "/api/graph/sections", body: { session: graph.session, gesture } });
}
