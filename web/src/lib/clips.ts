// Clip gestures (8b.5, design-8b §7.1): the timeline's edits as the server's `POST
// /api/graph/clips`, one undo step each, and the clips selected in the timeline.
//
// A gesture names clips by id, or (a clip with no id of its own) by where it starts and its rank
// there, with the hash the window drew it with: the server refuses one that changed since, in
// words, with 409 (WRITE-12). The shell keeps "the clip Notes shows" per track; the selection here
// can hold several clips, across tracks.

import { create } from "zustand";
import { clipIndex, findLane, hasOwnId, laneWriting, sameClip, type ClipRef, type GraphState, type TimelineLane, type TimelineView } from "./graph";
import { recorded, type Step } from "./history";
import { token } from "./native";

/** A lane's address, as the server writes it: module and lane keys. */
export interface LaneAddr {
  module: string;
  lane: string;
}

/** A clip as a gesture names it (the server's `ClipRef`). */
export interface ServerRef extends LaneAddr {
  id?: string;
  /** Where it starts, and its rank among the lane's clips that start there (from 0). */
  at?: [number, number];
  /** Its hash as this window drew it: a clip changed since is refused. */
  sha?: string;
}

/** What the timeline does to clips, in beats (the server's `Gesture`, tagged by `op`). */
export type Gesture =
  | ({ op: "create"; start: number; length?: number } & LaneAddr)
  | { op: "move"; clips: ServerRef[]; by: number; to?: LaneAddr; copy?: boolean }
  | { op: "resize"; clip: ServerRef; start: number; length: number }
  | { op: "delete"; clips: ServerRef[] };

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
 * done). Refused, it throws `Refused` with the server's words.
 */
export async function send(graph: GraphState, gesture: Gesture, step: Step): Promise<Done> {
  if (sent.session !== graph.session) sent = { session: graph.session, revision: 0, pending: 0 };
  const q = sent;
  q.pending++;
  try {
    const res = await fetch("/api/graph/clips", {
      method: "POST",
      headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
      body: JSON.stringify({ session: graph.session, gesture }),
    });
    const json = await res.json().catch(() => ({}));
    if (!res.ok) throw new Refused(json.error ?? `HTTP ${res.status}`, res.status === 409);
    const done = json as Done;
    q.revision = Math.max(q.revision, done.revision);
    if (done.before !== done.after) recorded(graph.session, done.revision, step);
    return done;
  } finally {
    q.pending--;
  }
}

// ---------------------------------------------------------------- the selection

interface Selection {
  /** The selected clips, in the order picked: ⌫ deletes them, a drag moves them together. */
  picked: ClipRef[];
  /** Only these. */
  only: (refs: ClipRef[]) => void;
  /** ⇧-click: this one too. */
  add: (ref: ClipRef) => void;
  /** ⌘-click: in or out. */
  toggle: (ref: ClipRef) => void;
  clear: () => void;
}

export const useClipSelection = create<Selection>((set) => ({
  picked: [],
  only: (refs) => set({ picked: refs }),
  add: (ref) => set((s) => (s.picked.some((p) => sameClip(p, ref)) ? s : { picked: [...s.picked, ref] })),
  toggle: (ref) => set((s) => ({ picked: s.picked.some((p) => sameClip(p, ref)) ? s.picked.filter((p) => !sameClip(p, ref)) : [...s.picked, ref] })),
  clear: () => set((s) => (s.picked.length ? { picked: [] } : s)),
}));

/** Whether `ref` is selected in the timeline. */
export const isPicked = (picked: readonly ClipRef[], ref: ClipRef) => picked.some((p) => sameClip(p, ref));
