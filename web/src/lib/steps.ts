// Lanes of names (CARD-23, CARD-24): a choice's or a switch's automation, whose points hold names, or
// true and false, each held until the next. The point picked (⌫ deletes it), and a clip's points as
// they are being edited, drawn in place of its own until the timeline shows the edit (`landed`).

import { create } from "zustand";
import type { Choice, ClipRef } from "./graph";

/** A point of a lane of names: its beat in its clip, and its name, or true or false. */
export type StepPoint = [number, string | boolean];

interface Steps {
  /** `at`: the point as picked, so an edit from elsewhere (an undo) that moves the points unpicks it. */
  picked: { clip: ClipRef; index: number; at: StepPoint } | null;
  editing: { clip: ClipRef; points: StepPoint[]; landed?: number } | null;
}

export const useSteps = create<Steps>(() => ({ picked: null, editing: null }));

/** Which of `points` is picked: -1 when none is, or the one picked has changed since. */
export function pickedStep(picked: Steps["picked"], points: readonly StepPoint[], editing: boolean) {
  if (!picked) return -1;
  const p = points[picked.index];
  return p && (editing || (p[0] === picked.at[0] && p[1] === picked.at[1])) ? picked.index : -1;
}

/** What a point's value says: its option's label, else the name as written (one the choice no longer has). */
export function stepLabel(options: readonly Choice[], v: string | boolean) {
  return options.find((o) => o.value === v)?.label ?? String(v);
}

/** `points` with `p` put at its beat: in place of one there already, else added; sorted by beat. */
export function putStep(points: readonly StepPoint[], p: StepPoint): StepPoint[] {
  const out = points.filter((q) => Math.abs(q[0] - p[0]) > 1e-9);
  out.push(p);
  return out.sort((a, b) => a[0] - b[0]);
}
