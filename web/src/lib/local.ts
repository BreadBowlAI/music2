// The page's view settings, per song (Q16): the timeline's zoom, follow and track heights, and
// whatever else a view keeps for next time. The server keeps them in the song's own
// `.music2/view.json`, local to this machine and never shared (FMT-20), and gives them back in
// the state's `local` when the song opens. A read-only window keeps them in memory only.
//
// A change shows at once and is sent 300 ms after the last one; anything waiting is sent at once
// before the song closes and when the page goes away, so a quick close never loses it.

import { create } from "zustand";
import { readJson } from "./graph";
import { token } from "./native";
import { useStore } from "./store";

declare module "./graph" {
  interface GraphState {
    /** The page's view settings (Q16): `view.json` as read when the song opened, and what was set since. */
    local?: Record<string, unknown>;
  }
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};
/** How long after the last change the settings are sent. */
const WAIT = 300;

interface Local {
  /** The song they are for. */
  session: number | null;
  values: Record<string, unknown>;
}

/** The open song's settings: grid.ts and audition.ts follow it to read theirs again when another song opens. */
export const useLocalStore = create<Local>(() => ({ session: null, values: {} }));
/** Set, not sent yet: by key, null to take one out. */
let waiting: Record<string, unknown> = {};
let timer: ReturnType<typeof setTimeout> | null = null;

/** A song opened (or another one): its settings, as the server gave them. */
function seed() {
  const graph = useStore.getState().graph;
  const session = graph?.session ?? null;
  if (session === useLocalStore.getState().session) return;
  waiting = {};
  if (timer) clearTimeout(timer);
  timer = null;
  useLocalStore.setState({ session, values: { ...(graph?.local ?? {}) } });
}
useStore.subscribe(seed);
seed();

/** Send what waits, now. */
export function flushLocal(): Promise<void> {
  if (timer) clearTimeout(timer);
  timer = null;
  const { session } = useLocalStore.getState();
  const set = waiting;
  waiting = {};
  if (session === null || Object.keys(set).length === 0) return Promise.resolve();
  return fetch("/api/graph/view", {
    method: "POST",
    headers: { "content-type": "application/json", ...auth },
    body: JSON.stringify({ session, set }),
    keepalive: true,
  })
    .then(async (res) => {
      if (!res.ok) console.warn(`music2: view settings not kept: ${(await readJson(res)).error ?? res.status}`);
    })
    .catch(() => {});
}

window.addEventListener("pagehide", () => void flushLocal());

/** The setting `key` of the open song, or `fallback` while none is kept. */
export function useLocal<T>(key: string, fallback: T): T {
  return useLocalStore((s) => (key in s.values ? (s.values[key] as T) : fallback));
}

/** The setting `key` now, outside React. */
export function getLocal<T>(key: string, fallback: T): T {
  const { values } = useLocalStore.getState();
  return key in values ? (values[key] as T) : fallback;
}

/** Set `key` (null takes it out): shown at once, kept for the next time the song opens. */
export function setLocal(key: string, value: unknown) {
  const { values } = useLocalStore.getState();
  if (JSON.stringify(values[key] ?? null) === JSON.stringify(value)) return;
  const next = { ...values };
  if (value === null) delete next[key];
  else next[key] = value;
  useLocalStore.setState({ values: next });
  waiting[key] = value;
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => void flushLocal(), WAIT);
}
