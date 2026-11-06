// The Takes rows (VOX-7): a lane's comps, worked out from its clips and their file's note of takes,
// as the server's comp works them out (`crates/editor/src/comp.rs`, VOX-2 and VOX-3). Nothing here is
// saved: which tracks show their rows is the window's own, and opening them is no edit.

import { create } from "zustand";
import type { AudioClip, TimelineLane } from "./graph";
import { secondsAt, type Tempo } from "./peaks";

/** The most two clips of a comp overlap, and the most their places in a pass differ, in seconds (VOX-2). */
const REACH = 0.05;

/** One piece of a comp: the beats it plays alone (from the middle of one join to the next), its take (from 0), and its clip's index in the lane. */
export interface Part {
  lo: number;
  hi: number;
  take: number;
  index: number;
}

/** A comp: its file, where each take starts in it (seconds), its span in beats and its pieces, in time order. */
export interface Comp {
  asset: string;
  takes: number[];
  start: number;
  end: number;
  parts: Part[];
}

/** Where clip `c` reads in its file at song beat `beat`, in seconds: by its offset, or its warp markers as the engine reads them. */
export function reads(c: AudioClip, beat: number, tempo: Tempo): number {
  const w = c.warp ?? [];
  const b = beat - c.start;
  if (w.length === 0) return (c.offset ?? 0) + secondsAt(tempo, beat) - secondsAt(tempo, c.start);
  if (w.length === 1) return w[0][0] + secondsAt(tempo, beat) - secondsAt(tempo, c.start + w[0][1]);
  const after = w.findIndex((m) => m[1] > b);
  const i = Math.min(Math.max(after < 0 ? w.length : after, 1), w.length - 1);
  const [a, z] = [w[i - 1], w[i]];
  return a[0] + ((b - a[1]) * (z[0] - a[0])) / (z[1] - a[1]);
}

/** The take that holds `at`, seconds into the file: the last that starts at or before it. */
export function takeAt(takes: readonly number[], at: number): number {
  let k = 0;
  takes.forEach((t, i) => {
    if (t <= at + 1e-6) k = i;
  });
  return k;
}

/** Where in its pass clip `c` plays at song beat `beat`, in seconds. */
const placeIn = (c: AudioClip, beat: number, takes: readonly number[], tempo: Tempo) => {
  const at = reads(c, beat, tempo);
  return at - takes[takeAt(takes, at)];
};

/**
 * The comps of audio lane `lane`, as the server works them out (`pieces` in comp.rs, VOX-2): its
 * clips of each file whose note lists two takes or more, in rows keeping one pass's timeline: each
 * starts after the one before starts, ends after it ends, starts no later than it ends (overlapping
 * it by 50 ms at most), and plays the same place in its pass as the one before there, within 50 ms.
 * Reversed and pitched clips are no pieces (VOX-6).
 */
export function compsOf(lane: TimelineLane, tempo: Tempo): Comp[] {
  if (lane.kind !== "audio") return [];
  const out: Comp[] = [];
  const byFile = new Map<string, number[]>();
  lane.clips.forEach((c, i) => {
    const takes = lane.takes?.[c.asset] ?? [];
    if (takes.length < 2 || c.reverse || (c.pitch ?? 0) !== 0) return;
    byFile.set(c.asset, [...(byFile.get(c.asset) ?? []), i]);
  });
  for (const [asset, all] of byFile) {
    const takes = lane.takes?.[asset] ?? [];
    const order = [...all].sort((a, b) => lane.clips[a].start - lane.clips[b].start || a - b);
    const links = (a: AudioClip, b: AudioClip) =>
      b.start > a.start &&
      b.start + b.length > a.start + a.length &&
      b.start <= a.start + a.length + 1e-9 &&
      secondsAt(tempo, a.start + a.length) - secondsAt(tempo, b.start) <= REACH + 1e-9 &&
      Math.abs(placeIn(a, b.start, takes, tempo) - placeIn(b, b.start, takes, tempo)) <= REACH + 1e-9;
    let row: number[] = [];
    const flush = () => {
      if (!row.length) return;
      const clips = row.map((i) => lane.clips[i]);
      const cuts = [clips[0].start];
      for (let k = 1; k < clips.length; k++) {
        const end = clips[k - 1].start + clips[k - 1].length;
        // The middle of the overlap, in seconds: near enough in beats for a drawing.
        cuts.push(clips[k].start < end ? (clips[k].start + end) / 2 : clips[k].start);
      }
      const last = clips[clips.length - 1];
      cuts.push(last.start + last.length);
      const parts = clips.map((c, k) => ({ lo: cuts[k], hi: cuts[k + 1], index: row[k], take: takeAt(takes, reads(c, (cuts[k] + cuts[k + 1]) / 2, tempo)) }));
      out.push({ asset, takes, start: cuts[0], end: cuts[cuts.length - 1], parts });
      row = [];
    };
    for (const i of order) {
      if (row.length && !links(lane.clips[row[row.length - 1]], lane.clips[i])) flush();
      row.push(i);
    }
    flush();
  }
  return out.sort((a, b) => a.start - b.start);
}

/** The clip a take's row draws over comp `comp`: its first piece, pointed at take `take` and stretched over the comp, as the whole take would play there (a comp keeps one pass's timeline). */
export function takeClip(lane: TimelineLane, comp: Comp, take: number): AudioClip {
  const first = lane.clips[comp.parts[0].index] as AudioClip;
  const by = comp.takes[take] - comp.takes[comp.parts[0].take];
  return {
    ...first,
    start: comp.start,
    length: comp.end - comp.start,
    offset: (first.offset ?? 0) + by,
    warp: first.warp?.map(([s, b]) => [s + by, b] as [number, number]),
    fade_in: 0,
    fade_out: 0,
    gain_db: 0,
  };
}

/** The tracks whose Takes rows are open, by module key: the window's own, never saved. */
export const useTakesOpen = create<{ open: ReadonlySet<string>; toggle: (module: string) => void }>((set) => ({
  open: new Set(),
  toggle: (module) =>
    set((s) => {
      const open = new Set(s.open);
      if (!open.delete(module)) open.add(module);
      return { open };
    }),
}));
