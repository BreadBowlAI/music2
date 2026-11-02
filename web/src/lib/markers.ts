// The ruler's markers (Q13): Intro, Verse, Chorus… at bar lines, kept in `layout.json` and shared
// with the song. Each change writes the whole list, as one undo step (`POST /api/graph/markers`).

import { create } from "zustand";
import { readJson, type Meter } from "./graph";
import { barAt } from "./graph";
import { editing, recorded, type Step } from "./history";
import { token } from "./native";
import { useStore } from "./store";
import { http } from "./http";

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

/**
 * The list as just sent, drawn until the timeline shows something newer than `since`, the list the
 * server had when it was sent (`answered`: its write was answered).
 */
export const usePendingMarkers = create<{ pending: { list: Marker[]; since: Marker[]; answered: boolean } | null }>(() => ({ pending: null }));

// Once its write is answered, a state that shows other markers than those it was made from ends it.
useStore.subscribe((s) => {
  const p = usePendingMarkers.getState().pending;
  if (p?.answered && (!s.graph || !sameMarkers(markersOf(s.graph.timeline), p.since))) usePendingMarkers.setState({ pending: null });
});

/** Whether two lists are the same markers, by beat and name (never by which array they are). */
export const sameMarkers = (a: readonly Marker[], b: readonly Marker[]) =>
  a.length === b.length && a.every((m, i) => Math.abs(m.beat - b[i].beat) < 1e-6 && m.name === b[i].name);

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** The window's marker writes, one after another: each made from the one before's result. */
let queue: Promise<unknown> = Promise.resolve();

/**
 * Write the song's markers: the whole list, one undo step named `step` ("Chorus at bar 17", "moving
 * Chorus"), made from the list `base` the window drew (AGENT-2): the server refuses it, saying who,
 * when the markers changed since, so none is lost silently. Drawn at once; writes go one after
 * another; refused, the list goes back and a toast says why.
 */
export function writeMarkers(list: Marker[], step: Step, base: Marker[]) {
  const { graph, toast } = useStore.getState();
  if (!graph) return Promise.resolve();
  const sorted = [...list].sort((a, b) => a.beat - b.beat);
  const pending = { list: sorted, since: markersOf(graph.timeline), answered: false };
  usePendingMarkers.setState({ pending });
  const ours = editing();
  const write = queue.then(async () => {
    try {
      const res = await http("/api/graph/markers", {
        method: "POST",
        headers: { "content-type": "application/json", ...auth },
        body: JSON.stringify({ session: graph.session, markers: sorted, base }),
      });
      const r = await readJson(res);
      if (!res.ok) throw new Error(r.error ?? `HTTP ${res.status}`);
      if (r.before !== r.after) recorded(graph.session, r, step);
      if (usePendingMarkers.getState().pending === pending) usePendingMarkers.setState({ pending: { ...pending, answered: true } });
    } catch (error) {
      if (usePendingMarkers.getState().pending === pending) usePendingMarkers.setState({ pending: null });
      toast((error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, ""));
    } finally {
      ours.end();
    }
  });
  queue = write;
  return write;
}
