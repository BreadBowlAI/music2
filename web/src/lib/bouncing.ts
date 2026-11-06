// Bounce to audio (RENDER-40): a track's menu renders the track as `m2 bounce` does, adds an audio
// track right under it playing the file in a copy of its mixer channel, and mutes the track in the
// song, as one undo step, so the song sounds the same. The request answers once it is in the song;
// meanwhile the server says how far it is with `bounce` events (crates/server/src/bounce.rs), and
// Stop takes it back, leaving no file. It isn't one of this window's edits on their way while it
// renders: gestures go on, and an edit made meanwhile refuses the bounce, in words.

import { create } from "zustand";
import { on } from "./events";
import { http } from "./http";
import { readJson } from "./graph";
import { recorded } from "./history";
import { token } from "./native";

/** The bounce rendering now: its track's name and song folder, and how far it is (0..1); null when none is. */
export const useBounce = create<{ label: string | null; dir: string | null; progress: number | null; stopping: boolean }>(() => ({
  label: null,
  dir: null,
  progress: null,
  stopping: false,
}));

const idle = { label: null, dir: null, progress: null, stopping: false };

on("bounce", (msg: { label?: string; dir?: string; progress?: number; done?: boolean; error?: string }) => {
  if (typeof msg.progress === "number") useBounce.setState({ label: msg.label ?? null, dir: msg.dir ?? null, progress: msg.progress });
  if (msg.done || msg.error) useBounce.setState(idle);
});

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await http(`/api/graph${path}`, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

/** What a bounce answers: the edit, the new track, the track muted and the file. */
export type Bounced = { revision?: number; step?: number; moved?: string[]; module: string; label: string; muted: string; asset: string };

/** Bounce track `track` (called `name`) of the song with session `session`: answers once it is in the song (one undo step), or throws the server's words. */
export async function bounce(session: number, track: string, name: string): Promise<Bounced> {
  useBounce.setState({ label: name, progress: 0, stopping: false });
  try {
    const r = await post<Bounced>("/bounce", { session, track });
    recorded(session, r, { name: `bouncing ${name}`, structural: true });
    return r;
  } finally {
    useBounce.setState(idle);
  }
}

/** Stop the bounce rendering now: nothing is added, and its file goes. */
export async function stopBounce() {
  useBounce.setState({ stopping: true });
  await post<{ stopped: boolean }>("/bounce/stop", {});
}
