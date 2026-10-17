// The view settings kept per song (polish Q16): each view's grid and snap, the audition toggle,
// and (lanes B and D) the zoom, follow, track heights and hidden lanes. The server keeps them in the
// song's `.music2/view.json`: local to this machine, never in git, never an undo step. A new song
// starts with the defaults. The dock and the side browser stay per window, in this browser's
// storage (`shell.ts`).
//
// The page reads them from the open song's state (`local`) when the song changes, or when the
// server changes them itself (`local_stamp`), and sends each change at once. While one is on its
// way, the next ones wait and go together after it.

import { create } from "zustand";
import { readJson, type GraphState } from "./graph";
import { token } from "./native";
import { useStore } from "./store";

declare module "./graph" {
  interface GraphState {
    /** The song's view settings, from `.music2/view.json` (polish Q16). Absent from older servers. */
    local?: Record<string, unknown>;
    /** Moves when the server changed them itself: the page reads them again then. */
    local_stamp?: number;
  }
}

interface Local {
  /** The song they are of (its session), and the stamp they were read at. */
  session: number | null;
  stamp: number | null;
  values: Record<string, unknown>;
}

export const useLocalStore = create<Local>(() => ({ session: null, stamp: null, values: {} }));

/** Changes not sent yet, and the session they are for. */
let waiting: { session: number; set: Record<string, unknown> } | null = null;
/** The post on its way, if one is. */
let sending: Promise<void> | null = null;

/** Read the open song's settings again when the song changes, or the server's stamp moves. */
function seed({ graph: g }: { graph: GraphState | null }) {
  const now = useLocalStore.getState();
  if (!g) {
    if (now.session !== null) useLocalStore.setState({ session: null, stamp: null, values: {} });
    return;
  }
  const stamp = g.local_stamp ?? 0;
  if (now.session === g.session && now.stamp === stamp) return;
  // The page's own changes still on their way stay as they are (null: taken out).
  const values: Record<string, unknown> = { ...(g.local ?? {}) };
  for (const [k, v] of Object.entries(waiting?.session === g.session ? waiting.set : {})) {
    if (v === null) delete values[k];
    else values[k] = v;
  }
  useLocalStore.setState({ session: g.session, stamp, values });
}
seed(useStore.getState());
useStore.subscribe(seed);

/** A setting of the open song, or `fallback` while it has none. */
export function useLocal<T>(key: string, fallback: T): T {
  return useLocalStore((s) => (key in s.values ? (s.values[key] as T) : fallback));
}

/** The same, outside a component. */
export function getLocal<T>(key: string, fallback: T): T {
  const { values } = useLocalStore.getState();
  return key in values ? (values[key] as T) : fallback;
}

/** Keep a setting of the open song; null removes it (back to its default). Sent at once. */
export function setLocal(key: string, value: unknown) {
  const { session, values } = useLocalStore.getState();
  if (session === null) return;
  const next = { ...values };
  if (value === null) delete next[key];
  else next[key] = value;
  useLocalStore.setState({ values: next });
  if (waiting && waiting.session !== session) waiting = null;
  waiting = { session, set: { ...(waiting?.set ?? {}), [key]: value } };
  if (!sending) sending = send();
}

/** Send what waits, one post at a time, until nothing does. */
async function send(keepalive = false) {
  while (waiting) {
    const { session, set } = waiting;
    waiting = null;
    try {
      const res = await fetch("/api/graph/view", {
        method: "POST",
        headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
        body: JSON.stringify({ session, set }),
        keepalive,
      });
      if (!res.ok) {
        const why = (await readJson(res)).error ?? `HTTP ${res.status}`;
        // Another song open by now: these were the last one's, which is closed.
        if (useLocalStore.getState().session === session) useStore.getState().toast(`Couldn't keep the view settings: ${why}`);
      }
    } catch {
      // The connection is gone: the page says so already; the settings stay as shown.
    }
  }
  sending = null;
}

/**
 * Send what waits, and wait until it is there: before the song closes, another opens, Save As or
 * Rename (plan X37). A change made just before would otherwise be lost with the session.
 */
export async function flushLocal() {
  if (waiting && !sending) sending = send();
  await sending;
}

// The window closing or reloading: what waits goes on its way all the same.
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", () => {
    if (waiting && !sending) void send(true);
  });
}
