// Clip gestures (8b.5, design-8b §7.1): the timeline's edits as the server's `POST
// /api/graph/clips`, one undo step each, and the clips selected in the timeline.
//
// A gesture names clips by id, or (a clip with no id of its own) by where it starts and its rank
// there, with the hash the window drew it with: the server refuses one that changed since, in
// words, with 409 (WRITE-12). The shell keeps "the clip Notes shows" per track; the selection here
// can hold several clips, across tracks.

import { create } from "zustand";
import { clipIndex, findLane, hasOwnId, laneWriting, readJson, sameClip, type ClipRef, type GraphState, type TimelineLane, type TimelineView } from "./graph";
import { beatNow } from "./clock";
import { clean } from "./grid";
import { editing, recorded, type Editing, type Step } from "./history";
import { token } from "./native";

/** A lane's address, as the server writes it: module and lane keys. */
export interface LaneAddr {
  module: string;
  lane: string;
}

declare module "./graph" {
  // The automation lane that drives an input (8b.5): "Follows the Cutoff lane", Show lane.
  interface InputPort {
    lane?: LaneAddr;
  }
}

/** A clip as a gesture names it (the server's `ClipRef`). */
export interface ServerRef extends LaneAddr {
  id?: string;
  /** Where it starts, and its rank among the lane's clips that start there (from 0). */
  at?: [number, number];
  /** Its hash as this window drew it: a clip changed since is refused. */
  sha?: string;
}

/** A clip on the clipboard: the lane it was copied from, and the clip as the lane format writes it. */
export interface Copied extends LaneAddr {
  clip: Record<string, unknown>;
}

/** What the timeline does to clips, in beats (the server's `Gesture`, tagged by `op`). */
export type Gesture =
  | ({ op: "create"; start: number; length?: number } & LaneAddr)
  | { op: "move"; clips: ServerRef[]; by: number; to?: LaneAddr; copy?: boolean }
  | { op: "resize"; clip: ServerRef; start: number; length: number }
  | { op: "delete"; clips: ServerRef[] }
  /** Each clip that crosses `at` becomes two there. */
  | { op: "split"; clips: ServerRef[]; at: number }
  /** Notes clips of one lane become one. */
  | { op: "join"; clips: ServerRef[] }
  /**
   * Copies right after the selection, or after `range` (a marquee's, on bar lines): a span of a
   * bar or more lands on the next bar line, a shorter one on the next line of `grid`.
   */
  | { op: "duplicate"; clips: ServerRef[]; range?: [number, number]; grid?: number; times?: number }
  /** Copied clips at `at`, keeping their distances: onto lane `to` when it takes them all. */
  | { op: "paste"; clips: Copied[]; to?: LaneAddr; at: number }
  /** Every note's start onto the nearest line of `grid` beats. */
  | { op: "quantize"; clips: ServerRef[]; grid: number; ends?: boolean }
  /** An automation clip's points, `[beat in the clip, value]`, all of them. */
  | { op: "points"; clip: ServerRef; points: [number, number][] | [number, string | boolean][] }
  /** The roll's edit of one notes clip: all its notes, and its length. */
  | { op: "notes"; clip: ServerRef; notes: { pitch: number; start: number; length: number; velocity: number }[]; length?: number }
  /** An audio clip's volume (dB), fades (seconds), direction and Follow tempo: those given (lane A's A3, sx's D11). */
  | { op: "audio"; clip: ServerRef; gain_db?: number; fade_in?: number; fade_out?: number; reverse?: boolean; follow?: boolean };

/** The server's answer to a gesture: the edit, and the clips it put on a lane they weren't on. */
export interface Done {
  file: string;
  before: string;
  after: string;
  revision: number;
  made: (LaneAddr & { id: string })[];
}

/**
 * How the timeline names `ref` to the server, from `lane` as drawn: by its id when it has its own,
 * else by its start and rank there (Q11). `sha` is what the window drew; a clip gone gives null.
 */
export function serverRef(lane: TimelineLane, ref: ClipRef): ServerRef | null {
  const i = clipIndex(lane.clips, ref);
  if (i < 0) return null;
  const c = lane.clips[i];
  const sha = c.sha ? { sha: c.sha } : {};
  if (hasOwnId(lane.clips, i)) return { module: ref.module, lane: ref.lane, id: c.id!, ...sha };
  const rank = lane.clips.slice(0, i).filter((x) => x.start === c.start).length;
  return { module: ref.module, lane: ref.lane, at: [c.start, rank], ...sha };
}

/** `serverRef` for a clip of `timeline`, found by its lane. */
export function refIn(timeline: TimelineView, ref: ClipRef) {
  const lane = findLane(timeline, ref.module, ref.lane);
  return lane ? serverRef(lane, ref) : null;
}

/**
 * Where the playhead is, for an edit there (⌘E, ⌘V, Remove automation): to the 960th of a beat, as
 * sequencers count ticks. The engine counts in samples, so a playhead put on beat 40 reads 39.99999.
 */
export const playhead = () => clean(Math.round(beatNow() * 960) / 960);

/** Gestures of the open project on their way, and the newest revision one made. */
let sent = { session: 0, revision: 0, pending: 0 };

/**
 * Whether the timeline can't take a gesture now: one is on its way, or made a revision `graph`
 * doesn't show yet, or the roll's lane write is. A gesture names clips as the timeline draws them,
 * so it waits until the timeline shows the last edit.
 */
export const busy = (graph: GraphState) =>
  laneWriting(graph) || (sent.session === graph.session && (sent.pending > 0 || graph.view.revision < sent.revision));

/** Why a gesture was refused: the server's words. `changed`: a clip it names changed since (409). */
export class Refused extends Error {
  constructor(
    message: string,
    readonly changed: boolean,
  ) {
    super(message);
  }
}

/**
 * Send one gesture: one Tx, one undo step named `step` (no toast: what it did shows where it was
 * done). Refused, it throws `Refused` with the server's words. `via`: another route that writes it
 * and answers as `/api/graph/clips` does (a drag's lease, 8b.7).
 */
export async function send(graph: GraphState, gesture: Gesture, step: Step, via?: { path: string; body: unknown }, edit?: Editing): Promise<Done> {
  if (sent.session !== graph.session) sent = { session: graph.session, revision: 0, pending: 0 };
  const q = sent;
  q.pending++;
  // On its way: as part of `edit` (a drag let go, which ends it), else as one of its own.
  const ours = edit ?? editing();
  try {
    const res = await fetch(via?.path ?? "/api/graph/clips", {
      method: "POST",
      headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
      body: JSON.stringify(via?.body ?? { session: graph.session, gesture }),
    });
    const json = await readJson(res);
    if (!res.ok) throw new Refused(json.error ?? `HTTP ${res.status}`, res.status === 409);
    const done = json as Done;
    q.revision = Math.max(q.revision, done.revision);
    // A gesture that changed nothing says "no change" on both sides. One whose words read the same
    // before and after (a quantize: the same clip, its notes moved) is still a step.
    if (done.before !== done.after || done.before !== "no change") recorded(graph.session, done, step, ours);
    return done;
  } finally {
    q.pending--;
    if (!edit) ours.end();
  }
}

// ---------------------------------------------------------------- the selection

interface Selection {
  /** The selected clips, in the order picked: ⌫ deletes them, a drag moves them together. */
  picked: ClipRef[];
  /** A marquee's bars, `[start, end)` in beats, while its clips are the selection: ⌘D copies that span. */
  range: [number, number] | null;
  /** Only these (a marquee's, with its bars). */
  only: (refs: ClipRef[], range?: [number, number] | null) => void;
  /** ⇧-click: this one too. */
  add: (ref: ClipRef) => void;
  /** ⌘-click: in or out. */
  toggle: (ref: ClipRef) => void;
  clear: () => void;
  /** Only the clips `timeline` still has: an edit elsewhere (a join, an undo) may take some away. */
  prune: (timeline: TimelineView) => void;
}

export const useClipSelection = create<Selection>((set) => ({
  picked: [],
  range: null,
  only: (refs, range = null) => set({ picked: refs, range }),
  add: (ref) => set((s) => (s.picked.some((p) => sameClip(p, ref)) ? s : { picked: [...s.picked, ref], range: null })),
  toggle: (ref) =>
    set((s) => ({ picked: s.picked.some((p) => sameClip(p, ref)) ? s.picked.filter((p) => !sameClip(p, ref)) : [...s.picked, ref], range: null })),
  clear: () => set((s) => (s.picked.length || s.range ? { picked: [], range: null } : s)),
  prune: (timeline) =>
    set((s) => {
      const kept = s.picked.filter((p) => {
        const lane = findLane(timeline, p.module, p.lane);
        return !!lane && clipIndex(lane.clips, p) >= 0;
      });
      return kept.length === s.picked.length ? s : { picked: kept };
    }),
}));

/** Whether `ref` is selected in the timeline. */
export const isPicked = (picked: readonly ClipRef[], ref: ClipRef) => picked.some((p) => sameClip(p, ref));

/** The selected clips as the timeline draws them now, each with its lane; gone ones left out. */
export function pickedIn(timeline: TimelineView, picked: readonly ClipRef[]) {
  return picked.flatMap((ref) => {
    const lane = findLane(timeline, ref.module, ref.lane);
    const i = lane ? clipIndex(lane.clips, ref) : -1;
    return lane && i >= 0 ? [{ ref, lane, clip: lane.clips[i] as TimelineLane["clips"][number] }] : [];
  });
}

// ---------------------------------------------------------------- the clipboard

/**
 * The clips ⌘C or ⌘X took, as the lane format writes them, in this window (not the system's
 * clipboard: a copied clip is no text to paste anywhere else). ⌘V puts them at the playhead.
 */
export const useClipboard = create<{ copied: Copied[] }>(() => ({ copied: [] }));

/** `picked` as the clipboard keeps them: each clip without its hash, from its lane. */
export function copiedOf(timeline: TimelineView, picked: readonly ClipRef[]): Copied[] {
  return pickedIn(timeline, picked).map(({ ref, clip }) => {
    const { sha: _sha, ...rest } = clip as unknown as Record<string, unknown>;
    return { module: ref.module, lane: ref.lane, clip: rest };
  });
}

// ---------------------------------------------------------------- remove automation

/**
 * Remove automation (HOME-2): the lane's read in the code becomes what it plays at `beat` (the
 * playhead), and its file goes. One undo step, named "removing the Cutoff automation". Refused,
 * it throws the server's words.
 */
export async function removeAutomation(graph: GraphState, addr: LaneAddr, beat: number, name: string) {
  const ours = editing();
  try {
    const res = await fetch("/api/graph/automate/remove", {
      method: "POST",
      headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
      body: JSON.stringify({ session: graph.session, module: addr.module, lane: addr.lane, beat }),
    });
    const json = await readJson(res);
    if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
    const done = json as { file: string; before: string; after: string; revision: number; step?: number };
    if (done.before !== done.after) recorded(graph.session, done, { name: `removing the ${name} automation`, structural: true }, ours);
    return done;
  } finally {
    ours.end();
  }
}
