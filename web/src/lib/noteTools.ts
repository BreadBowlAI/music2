// The roll's note tools (D1): transpose, quantize, legato, humanize and swing. Each is pure: it
// takes the clip's notes and the indices it acts on, and gives the notes after, or null when
// nothing would change. The roll writes the result as one lane write, so one undo step.
//
// What a tool acts on (the plan's Q25): the selected notes; with none selected, every note.

import { snapTo } from "./grid";
import type { Meter, Note } from "./graph";

/** The highest pitch; the lowest is 0. */
const TOP = 127;
/** The shortest a note gets, in beats. */
const MIN_LEN = 1 / 64;

/** Beats and lengths as the roll writes them: to a millionth of a beat, never -0. */
const clean = (x: number) => Math.round(x * 1e6) / 1e6 + 0;
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const same = (a: Note[], b: Note[]) => a.length === b.length && a.every((n, i) => n.pitch === b[i].pitch && n.start === b[i].start && n.length === b[i].length && n.velocity === b[i].velocity);

/** The notes a tool acts on: the picked ones, or every note when none is picked. */
export const targets = (notes: readonly Note[], picked: readonly number[]) => (picked.length ? [...picked] : notes.map((_, i) => i));

/** `notes` with the notes at `which` changed by `f`; null when nothing changed. */
function change(notes: readonly Note[], which: readonly number[], f: (n: Note, i: number) => Note): Note[] | null {
  const set = new Set(which);
  const out = notes.map((n, i) => (set.has(i) ? f(n, i) : n));
  return same(out, notes as Note[]) ? null : out;
}

/**
 * Move the notes `by` semitones. The notes move together: when some would leave the keyboard,
 * they all move only as far as the last one can.
 */
export function transpose(notes: readonly Note[], picked: readonly number[], by: number): Note[] | null {
  const which = targets(notes, picked);
  if (!which.length) return null;
  const pitches = which.map((i) => notes[i].pitch);
  const d = clamp(by, -Math.min(...pitches), TOP - Math.max(...pitches));
  if (!d) return null;
  return change(notes, which, (n) => ({ ...n, pitch: n.pitch + d }));
}

/**
 * Move each note's start to the nearest line of a grid `step` beats wide (Q). The grid follows
 * the song's bars, as the roll's own lines do; `start` is where the clip starts in the song.
 * Lengths stay as they are.
 */
export function quantize(notes: readonly Note[], picked: readonly number[], step: number, start: number, meter: readonly Meter[]): Note[] | null {
  if (!(step > 0)) return null;
  return change(notes, targets(notes, picked), (n) => ({ ...n, start: clean(Math.max(0, snapTo(meter, start + n.start, step) - start)) }));
}

/**
 * Legato: each note lasts until the next note starts. The notes of a chord (one start) move
 * together. The last notes keep their length: nothing comes after them.
 */
export function legato(notes: readonly Note[], picked: readonly number[]): Note[] | null {
  const which = targets(notes, picked);
  const starts = [...new Set(which.map((i) => notes[i].start))].sort((a, b) => a - b);
  return change(notes, which, (n) => {
    const next = starts.find((s) => s > n.start + 1e-9);
    return next === undefined ? n : { ...n, length: clean(Math.max(next - n.start, MIN_LEN)) };
  });
}

/** How far humanize moves a note: its start up to this many ms either way. */
export const HUMANIZE_MS = 10;
/** How far humanize moves a note's velocity, up or down (8% of full). */
export const HUMANIZE_VELOCITY = 0.08;

/**
 * Humanize: each note's start moves a little (up to ±10 ms at the song's tempo where it is) and
 * its velocity a little (up to ±8%), so the part sounds played. `beatsPerMs(beat)` is how many
 * beats a millisecond is at clip beat `beat`; `random` gives numbers in [0, 1).
 */
export function humanize(
  notes: readonly Note[],
  picked: readonly number[],
  beatsPerMs: (beat: number) => number,
  random: () => number = Math.random,
): Note[] | null {
  const spread = () => random() * 2 - 1;
  return change(notes, targets(notes, picked), (n) => {
    // Rounded as the roll writes them: to a thousandth of a beat would lose a 10 ms move at a slow tempo, so to a millionth.
    const start = clean(Math.max(0, n.start + spread() * HUMANIZE_MS * beatsPerMs(n.start)));
    const velocity = Math.round(clamp(n.velocity + spread() * HUMANIZE_VELOCITY, 0.01, 1) * 100) / 100;
    return { ...n, start, velocity };
  });
}

/** Swing's three amounts (Q26): where the second note of each pair goes, as a share of the pair. */
export const SWINGS = { light: 0.55, medium: 0.6, heavy: 0.66 } as const;
export type Swing = keyof typeof SWINGS;

/** What swing pairs: eighths when the roll's grid is 1/8 or wider, else sixteenths (Q26). */
export const swingUnit = (gridStep: number) => (gridStep >= 0.5 - 1e-9 ? 0.5 : 0.25);

/**
 * Swing: the second note of each pair of eighths (or sixteenths, `unit` beats each) goes later,
 * to `amount` of the pair, for a bouncy feel. It sets those notes to their swung place, so doing
 * it twice changes nothing, and Light after Heavy puts them back to Light's place. A note is the
 * second of its pair when it starts near the pair's middle: from a quarter of a step before it to
 * just past where Heavy puts it. Notes elsewhere stay. A moved note keeps its end, so it never
 * runs into the next one. Pairs follow the song's beats; `start` is where the clip starts.
 */
export function swing(notes: readonly Note[], picked: readonly number[], amount: number, unit: number, start: number): Note[] | null {
  const pair = 2 * unit;
  return change(notes, targets(notes, picked), (n) => {
    const at = start + n.start;
    const from = Math.floor(at / pair + 1e-9) * pair;
    const into = (at - from) / unit;
    // Straight is 1 step into the pair; Heavy (66%) puts it at 1.32.
    if (into < 0.75 - 1e-9 || into > 1.35 + 1e-9) return n;
    const to = clean(from + pair * amount - start);
    if (to === n.start) return n;
    const end = n.start + n.length;
    return { ...n, start: to, length: clean(Math.max(end - to, MIN_LEN)) };
  });
}
