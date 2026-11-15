// What the arrangement draws by, which both the page (SongTimeline.tsx) and its zoom picture
// (components/zoomPicture/) read, so the two never disagree (taste.md VIEW-13): the song's room, a
// track's rows, the tempo and time signature changes, the grid's step, a clip's audible notes and
// a bus's words.

import { automation, barAt, kitOf, moduleName, type GraphState, type Meter, type NoteClip, type TimelineTrack, type TimelineView } from "./graph";
import { stepOf, type Grid } from "./grid";
import { opt } from "./native";

const beats = (n: number) => Number(n.toFixed(3));

/** The part of each note that plays: cut to its clip, as playback cuts it. Notes wholly outside are silent. */
export function audible(clip: NoteClip) {
  return clip.notes.flatMap((n) => {
    const start = Math.max(n.start, 0), end = Math.min(n.start + n.length, clip.length);
    return end > start ? [{ ...n, start, length: end - start }] : [];
  });
}

/** A clip's audible notes, the same array while the clip is the same: its drawing is kept by it. */
const audibles = new WeakMap<NoteClip, NoteClip["notes"]>();
export const audibleOf = (c: NoteClip) => audibles.get(c) ?? audibles.set(c, audible(c)).get(c)!;

/** The tempo and time signature changes after the start, by beat. */
export function changes(timeline: TimelineView) {
  const at = new Map<number, { beat: number; bpm?: number; meter?: string }>();
  const get = (beat: number) => at.get(beat) ?? at.set(beat, { beat }).get(beat)!;
  for (const t of timeline.tempo.slice(1)) get(t.beat).bpm = beats(t.bpm);
  for (const m of timeline.meter.slice(1)) get(m.beat).meter = `${m.beats}/${m.unit}`;
  return [...at.values()].sort((a, b) => a.beat - b.beat);
}

/** A change's words on the ruler: "3/4 ♩140". */
export const changeWords = (c: { meter?: string; bpm?: number }) => [c.meter, c.bpm && `♩${c.bpm}`].filter(Boolean).join(" ");

/** Where the timeline ends: a bar after the bar the last clip, or the playhead, is in. */
export function room(meter: Meter[], end: number, beat: number) {
  // A clip ending on a bar line is in the bar before it.
  const last = barAt(meter, Math.max(end, beat, 1e-6) - 1e-6);
  const next = last.start + last.length;
  return next + barAt(meter, next).length;
}

/**
 * A track's rows (taste.md §3.4): its own row draws its primary lane, the first notes or audio
 * lane in code order; its other notes and audio lanes are full rows under it, always shown; its
 * automation lanes are 32px rows under those, folded until asked for (A).
 */
export function rowsOf(track: TimelineTrack) {
  const primary = track.lanes.find((l) => !automation(l)) ?? null;
  return {
    primary,
    others: track.lanes.filter((l) => l !== primary && !automation(l)),
    controls: track.lanes.filter(automation),
  };
}

/** The ruler's tools: 3 buttons of 24 px, 2 px apart, 4 px in, and the 1 px edge. */
export const TOOLS_W = 3 * 24 + 2 * 2 + 2 * 4 + 1;

/** The closest beat lines the timeline draws, in px: zoomed out further, bars only. */
export const BEAT_LINES = 6;

/** The grid's step where the timeline draws its lines, in a bar of `bar` beats at `ppb`; null where it doesn't. */
export function fineStep(grid: Grid, bar: number, ppb: number) {
  const step = stepOf(grid, ppb, bar);
  const fits = Math.abs(bar / step - Math.round(bar / step)) < 1e-6;
  return step < 1 && step * ppb >= 12 && fits ? step : null;
}

/**
 * What a bus's row says, as it has no lanes (RENDER-34): how tracks reach it, by their output or a
 * send; for a drum kit's bus, that it is the kit, which drums play into it and that Beat writes their
 * beat (KIT-10).
 */
export function busWords(graph: GraphState | null | undefined, module: string): string {
  const kit = graph && kitOf(graph, module);
  if (!graph || !kit || kit.bus !== module) return `Plays the tracks whose output is set to it, and what tracks send it. Set them in Mix (${opt("3")}).`;
  const names = kit.rows.map((r) => moduleName({ key: r.track, label: graph.timeline.tracks.find((t) => t.module === r.track)?.label }));
  const drums = names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)} play` : `${names[0]} plays`;
  return `The ${kit.label} kit: ${drums} into it. Write ${names.length > 1 ? "their" : "its"} beat in Beat (${opt("5")}).`;
}
