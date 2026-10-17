// Songs (polish lane C): the recent songs list, and the calls of Save As and Rename Song. The list
// is the server's (`~/.music2/recent.json`): opening a song any way puts it first.

import { readJson } from "./graph";
import { token } from "./native";

/** A song of the recent list, read from its folder when listed. */
export interface RecentSong {
  /** Its folder. */
  path: string;
  name: string;
  /** Its first tempo; null when its folder is gone. */
  tempo: number | null;
  /** When it was last opened, in UTC (RFC 3339). */
  opened: string;
  /** Its folder is gone: "Moved or deleted". */
  missing: boolean;
}

/** GET, or POST `body` as JSON; an error answer throws with the server's reason. */
export async function request<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await readJson(res);
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

export const recentApi = {
  list: () => request<{ songs: RecentSong[] }>("/api/project/recent").then((r) => r.songs),
  /** Take a song off the list (its × while its folder is gone). Answers with the list. */
  forget: (path: string) => request<{ songs: RecentSong[] }>("/api/project/recent/forget", { path }).then((r) => r.songs),
};

const DAY = 24 * 60 * 60 * 1000;

/** When a song was opened, in words: "opened today", "opened yesterday", "opened 3 days ago", "opened 12 Sep". */
export function openedWords(iso: string, now = new Date()): string {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return "opened before";
  const midnight = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((midnight(now) - midnight(then)) / DAY);
  if (days <= 0) return "opened today";
  if (days === 1) return "opened yesterday";
  if (days < 7) return `opened ${days} days ago`;
  const sameYear = then.getFullYear() === now.getFullYear();
  return `opened ${then.toLocaleDateString("en-GB", { day: "numeric", month: "short", ...(sameYear ? {} : { year: "numeric" }) })}`;
}

/** A tempo as the list says it: "124 BPM", "92.5 BPM". */
export const bpmWords = (bpm: number) => `${Math.round(bpm * 10) / 10} BPM`;
