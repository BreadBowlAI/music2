// The song's key (the polish plan's D2, Q22): `layout.json`'s `key`, shown in the top bar and as
// shading in Notes, which can lock notes to it and stamp its chords. Nothing that plays reads it.
//
// The page knows no music of its own (NOTE-2): the scales, chords and grooves are the server's one
// table (`GET /api/theory`, NOTE-1), and what the song's key means (its notes, the nearest of them
// to each pitch, its chords) comes worked out with the timeline. The page only looks them up.

import { create } from "zustand";
import { readJson } from "./graph";
import { editing, recorded } from "./history";
import { token } from "./native";

/** A key as `layout.json` writes it: its root ("C#", "Eb") and its scale by name ("harmonic-minor"). */
export interface SongKey {
  root: string;
  scale: string;
}

/** One chord on a note of the key: its name ("Dm") and its notes for each inversion, in semitones from the root's row. */
export interface Stamp {
  name: string;
  shapes: number[][];
}

/** The chords on one note of the key (NOTE-4). */
export interface Degree {
  /** Its root's pitch class, 0 is C. */
  root: number;
  /** Its place in the key: "IV", "ii", "vii°". */
  numeral: string;
  /** Each kind of chord on it, by the kind's name ("triad", "seventh"…). */
  kinds: Record<string, Stamp>;
}

/** The song's key as the timeline publishes it (NOTE-2): the key, and what it means. */
export interface KeyView extends SongKey {
  /** As the app says it: "E♭ major", "D Dorian". */
  name: string;
  /** Its pitch classes, from the root up. */
  notes: number[];
  /** For each pitch class from C, the semitones to the nearest note of the key (a tie goes down). */
  snap: number[];
  /** The chords on its notes, from the root up; none for a key with no chords (chromatic). */
  chords: Degree[];
}

declare module "./graph" {
  interface TimelineView {
    /** The song's key, from `layout.json`, with what it means; null when none is set. */
    key?: KeyView | null;
  }
}

/** The twelve roots as the file writes them, from C. */
export const ROOTS = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"] as const;

/** A root as the app spells it (Q32): C♯, E♭. */
export const rootName = (root: string) => (root.length === 2 ? `${root[0]}${root[1] === "#" ? "♯" : "♭"}` : root);

/** The pitch classes (0 is C) in key `k`. */
export const inKey = (k: KeyView): Set<number> => new Set(k.notes);

/** The highest pitch; the lowest is 0. */
const TOP = 127;
const pc = (p: number) => ((p % 12) + 12) % 12;

/** The note of key `k` next above (`dir` 1) or below (-1) pitch `p`, for ↑ and ↓ with Lock to key on; `p` itself at the keyboard's end. */
export function stepIn(k: KeyView, p: number, dir: 1 | -1): number {
  const notes = inKey(k);
  for (let q = p + dir; q >= 0 && q <= TOP; q += dir) if (notes.has(pc(q))) return q;
  return p;
}

/**
 * Pitch `p` at the nearest note of key `k` (NOTE-3), by the key's own table: a pitch exactly between
 * two notes of the key goes down. Where the table points past an end of the keyboard, the key's
 * nearest note on it, the other way, as the server's snap does.
 */
export function fit(k: KeyView, p: number): number {
  const q = Math.round(p);
  const to = q + k.snap[pc(q)];
  if (to >= 0 && to <= TOP) return to;
  return to > TOP ? stepIn(k, TOP + 1, -1) : stepIn(k, -1, 1);
}

/**
 * The chord of kind `kind` in inversion `inversion` that a stamp on row `row` places (NOTE-4): its
 * root is the row's nearest note of the key. Its name, numeral and pitches; null where the key has
 * no such chord (no key chords, the blues' blue note) or it would go past the top.
 */
export function chordAt(k: KeyView, row: number, kind: string, inversion: number): { name: string; numeral: string; pitches: number[] } | null {
  const root = fit(k, row);
  const d = k.chords.find((c) => c.root === pc(root));
  const stamp = d?.kinds[kind];
  if (!d || !stamp) return null;
  const pitches = stamp.shapes[inversion % stamp.shapes.length].map((o) => root + o);
  return pitches.every((p) => p <= TOP) ? { name: stamp.name, numeral: d.numeral, pitches } : null;
}

// ---------------------------------------------------------------- the table (NOTE-1)

/** A scale of the table. */
export interface Scale {
  name: string;
  label: string;
  /** What it sounds like, in a few words. */
  words: string;
  steps: number[];
}
/** A chord a stamp places, or a groove: its name in a request, its label and what it does. */
export interface Named {
  name: string;
  label: string;
  words: string;
}
/** What music2 knows of scales, chords and grooves: `GET /api/theory`. */
export interface Theory {
  scales: Scale[];
  chords: (Named & { inversions: number })[];
  grooves: Named[];
  /** The swings the roll offers, as shares of a pair: 0.5 is straight. */
  swings: number[];
}

/** The table, once read; null until then. */
export const useTheory = create<{ theory: Theory | null }>(() => ({ theory: null }));

let asked: Promise<void> | null = null;
/** Read the table, once per page; what lists it shows it when it lands. */
export function loadTheory() {
  asked ??= fetch("/api/theory", { headers: token ? { "x-m2-token": token } : {} })
    .then(async (res) => {
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      useTheory.setState({ theory: (await readJson(res)) as Theory });
    })
    .catch(() => void (asked = null));
  return asked;
}

/**
 * Set the song's key, or with null take it out: a write of `layout.json` alone (`POST
 * /api/graph/song-key`), one undo step, named "key A minor". Refused, it throws with the server's
 * words.
 */
export async function setSongKey(session: number, key: SongKey | null) {
  const ours = editing();
  try {
    const res = await fetch("/api/graph/song-key", {
      method: "POST",
      headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
      body: JSON.stringify({ session, key }),
    });
    const done = await readJson(res);
    if (!res.ok) throw new Error(done.error ?? `HTTP ${res.status}`);
    // Setting the key it has already changes nothing, and is no step. The server names the key.
    if (done.before !== "no change") recorded(session, done, { name: key ? `key ${done.after}` : "taking out the key" }, ours);
    return done as { revision: number; before: string; after: string };
  } finally {
    ours.end();
  }
}
