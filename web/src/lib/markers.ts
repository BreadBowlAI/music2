// The ruler's markers (Q13): Intro, Verse, Chorus… at bar lines, kept in `layout.json` and shared
// with the song. Each change writes the whole list, as one undo step (`POST /api/graph/markers`).

import { create } from "zustand";
import { readJson, type Meter } from "./graph";
import { barAt } from "./graph";
import { recorded, type Step } from "./history";
import { token } from "./native";
import { useStore } from "./store";

/** A marker: where, in beats, and its name (one line, 1 to 40 characters). */
export interface Marker {
  beat: number;
  name: string;
}

declare module "./graph" {
  interface TimelineView {
    /** The ruler's markers, by beat, from `layout.json`: none while it doesn't read. Absent from older servers. */
    markers?: Marker[];
  }
}

/** The names offered under a new marker's field. */
export const PICKS = ["Intro", "Verse", "Pre-chorus", "Chorus", "Bridge", "Drop", "Outro"];
/** The longest name. */
export const NAME_MAX = 40;

const NONE: Marker[] = [];
/** The markers of `timeline`, by beat. */
export const markersOf = (timeline: { markers?: Marker[] }) => timeline.markers ?? NONE;

const same = (a: number, b: number) => Math.abs(a - b) < 1e-6;
/** The marker on beat `beat`, if one is. */
export const markerAt = (list: Marker[], beat: number) => list.find((m) => same(m.beat, beat));

/** "Marker 1", or the next number no marker's name has. */
export function nextName(list: Marker[]) {
  const used = new Set(list.map((m) => /^Marker (\d+)$/.exec(m.name)?.[1]).filter(Boolean).map(Number));
  let n = 1;
  while (used.has(n)) n++;
  return `Marker ${n}`;
}

/** The bar a beat is in, counted from 1. */
export const barOf = (meter: Meter[], beat: number) => barAt(meter, beat).bar + 1;

/** The last marker before `beat` (⌥←), and the first after it (⌥→). */
export const before = (list: Marker[], beat: number) => list.findLast((m) => m.beat < beat - 1e-3);
export const after = (list: Marker[], beat: number) => list.find((m) => m.beat > beat + 1e-3);

/**
 * The name field open on the ruler (`beat`): a new marker's while `from` is null, written once
 * when its name is kept (X19); else `from`'s, to rename it.
 */
export const useMarkerEdit = create<{ editing: { beat: number; from: Marker | null } | null }>(() => ({ editing: null }));

/** The list as just sent, drawn until the timeline shows it (`since`: the list it was made from). */
export const usePendingMarkers = create<{ pending: { list: Marker[]; since: Marker[] } | null }>(() => ({ pending: null }));

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/**
 * Write the song's markers: the whole list, one undo step named `step` ("Chorus at bar 17", "moving
 * Chorus"). Drawn at once; refused, the list goes back and a toast says why.
 */
export async function writeMarkers(list: Marker[], step: Step) {
  const { graph, toast } = useStore.getState();
  if (!graph) return;
  const since = markersOf(graph.timeline);
  const sorted = [...list].sort((a, b) => a.beat - b.beat);
  usePendingMarkers.setState({ pending: { list: sorted, since } });
  try {
    const res = await fetch("/api/graph/markers", {
      method: "POST",
      headers: { "content-type": "application/json", ...auth },
      body: JSON.stringify({ session: graph.session, markers: sorted }),
    });
    const r = await readJson(res);
    if (!res.ok) throw new Error(r.error ?? `HTTP ${res.status}`);
    if (r.before !== r.after) recorded(graph.session, r, step);
  } catch (error) {
    usePendingMarkers.setState({ pending: null });
    toast((error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, ""));
  }
}
