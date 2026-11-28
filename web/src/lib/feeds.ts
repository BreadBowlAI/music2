// Feeds (FEED-1, FEED-7): a track an agent plays live, its notes written ahead of the playhead.
// The page opens one from the track's menu (Let an agent play this track), shows it as the Live mark
// on the track's header with the writer's name, and offers Keep what played and Close there.
//
// Opening and closing are playback, not edits: no undo step, no revision, and they work in a
// read-only window too. Keep is an edit: one undo step, which its toast's Undo takes back.

import { readJson, type GraphState } from "./graph";
import { editing } from "./history";
import { http } from "./http";
import { token } from "./native";
import { useStore } from "./store";

/** An open feed, as the graph state lists it. */
export interface FeedState {
  feed: string;
  track: string;
  lane: string;
  /** The agent that opened it or wrote to it last; null until an agent writes. */
  writer: string | null;
  low_bars: number;
}

/** What Keep answers: how many notes it kept, and how (a new clip, or a version), with its step. */
export interface Kept {
  kept: number;
  as: "clip" | "version";
  version?: string;
  step?: number;
  revision?: number;
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await http(`/api/feed${path}`, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

const NONE: FeedState[] = [];

/** The feeds the state lists. */
export const feedsOf = (graph: GraphState | null | undefined): FeedState[] => (graph as (GraphState & { feeds?: FeedState[] }) | null | undefined)?.feeds ?? NONE;

/** `module`'s open feed, or undefined. */
export function useFeed(module: string): FeedState | undefined {
  return useStore((s) => feedsOf(s.graph).find((f) => f.track === module));
}

/** Whether `module` can be played live: a track (not a bus) with a notes lane. */
export function feedable(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((x) => x.module === module);
  return !!t && t.track && !t.bus && t.lanes.some((l) => l.kind === "notes");
}

/** Open a feed on `module` (the track's menu): its Live mark shows once the state says so. */
export async function openFeed(session: number, module: string) {
  return post<unknown>("/open", { session, track: module });
}

/** Close `module`'s feed: its notes end at once. */
export async function closeFeed(session: number, module: string) {
  return post<unknown>("/close", { session, feed: module });
}

/** Keep what played on `module`'s feed: one undo step. */
export async function keepFeed(session: number, module: string): Promise<Kept> {
  const ours = editing();
  try {
    return await post<Kept>("/keep", { session, feed: module });
  } finally {
    ours.end();
  }
}
