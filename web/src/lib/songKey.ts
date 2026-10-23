// The song's key (the polish plan's D2, Q22): `layout.json`'s `key`, shown in the top bar and as
// shading in Notes. Nothing that plays reads it, and nothing is locked to it.

import { readJson } from "./graph";
import { recorded } from "./history";
import { token } from "./native";

/** A key: its root as `layout.json` writes it ("C#", "Eb") and its scale. */
export interface SongKey {
  root: string;
  scale: "major" | "minor";
}

declare module "./graph" {
  interface TimelineView {
    /** The song's key, from `layout.json`; null when none is set. */
    key?: SongKey | null;
  }
}

/** The twelve roots as the file writes them, from C. */
export const ROOTS = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"] as const;

/** A root as the app spells it (Q32): C♯, E♭. */
export const rootName = (root: string) => (root.length === 2 ? `${root[0]}${root[1] === "#" ? "♯" : "♭"}` : root);

/** A key's name: "A minor", "E♭ major". */
export const songKeyName = (k: SongKey) => `${rootName(k.root)} ${k.scale}`;

const STEPS = { major: [0, 2, 4, 5, 7, 9, 11], minor: [0, 2, 3, 5, 7, 8, 10] } as const;

/** The pitch classes (0 is C) in key `k`. */
export function inKey(k: SongKey): Set<number> {
  const root = ROOTS.indexOf(k.root as (typeof ROOTS)[number]);
  return new Set(STEPS[k.scale].map((i) => (Math.max(0, root) + i) % 12));
}

/**
 * Set the song's key, or with null take it out: a write of `layout.json` alone (`POST
 * /api/graph/song-key`), one undo step, named "key A minor". Refused, it throws with the server's
 * words.
 */
export async function setSongKey(session: number, key: SongKey | null) {
  const res = await fetch("/api/graph/song-key", {
    method: "POST",
    headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
    body: JSON.stringify({ session, key }),
  });
  const done = await readJson(res);
  if (!res.ok) throw new Error(done.error ?? `HTTP ${res.status}`);
  // Setting the key it has already changes nothing, and is no step.
  if (done.before !== "no change") recorded(session, done, { name: key ? `key ${songKeyName(key)}` : "taking out the key" });
  return done as { revision: number; before: string; after: string };
}
