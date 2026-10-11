import { PPQ, type ScaleName, type Song } from "./types";

export const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

export const SCALES: { id: ScaleName; name: string; intervals: number[] }[] = [
  { id: "major", name: "major", intervals: [0, 2, 4, 5, 7, 9, 11] },
  { id: "minor", name: "minor", intervals: [0, 2, 3, 5, 7, 8, 10] },
  { id: "dorian", name: "dorian", intervals: [0, 2, 3, 5, 7, 9, 10] },
  { id: "mixolydian", name: "mixolydian", intervals: [0, 2, 4, 5, 7, 9, 10] },
  { id: "lydian", name: "lydian", intervals: [0, 2, 4, 6, 7, 9, 11] },
  { id: "phrygian", name: "phrygian", intervals: [0, 1, 3, 5, 7, 8, 10] },
  { id: "harmonic_minor", name: "harmonic minor", intervals: [0, 2, 3, 5, 7, 8, 11] },
  { id: "pentatonic_major", name: "major pentatonic", intervals: [0, 2, 4, 7, 9] },
  { id: "pentatonic_minor", name: "minor pentatonic", intervals: [0, 3, 5, 7, 10] },
  { id: "chromatic", name: "chromatic", intervals: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] },
];

export const noteName = (p: number) => `${NOTE_NAMES[p % 12]}${Math.floor(p / 12) - 1}`;
export const keyName = (song: Song) => `${NOTE_NAMES[song.key.root]} ${SCALES.find((s) => s.id === song.key.scale)?.name ?? song.key.scale}`;

export function inKey(song: Song, pitch: number) {
  const iv = SCALES.find((s) => s.id === song.key.scale)?.intervals ?? [];
  return iv.includes((((pitch - song.key.root) % 12) + 12) % 12);
}

export const ticksPerBeat = (song: Song) => (PPQ * 4) / song.time_sig.den;
export const ticksPerBar = (song: Song) => ticksPerBeat(song) * song.time_sig.num;
export const ticksToBeats = (song: Song, t: number) => t / ticksPerBeat(song);
export const beatsToTicks = (song: Song, b: number) => Math.round(b * ticksPerBeat(song));

export function position(song: Song, t: number) {
  const bar = Math.floor(t / ticksPerBar(song)) + 1;
  const beat = Math.floor((t % ticksPerBar(song)) / ticksPerBeat(song)) + 1;
  return `${bar}.${beat}`;
}

/** Track color, muted toward gray to fit the light theme. */
const mute = (v: number) => Math.round(v * 0.5 + 155 * 0.5);
export const rgb = (c: [number, number, number], a = 1) => `rgba(${mute(c[0])},${mute(c[1])},${mute(c[2])},${a})`;

export function songEnd(song: Song) {
  let end = ticksPerBar(song);
  for (const t of song.tracks) for (const c of t.clips) end = Math.max(end, c.start + c.length);
  return end;
}

export function findClip(song: Song, id: number | null) {
  if (id == null) return null;
  for (const t of song.tracks) for (const c of t.clips) if (c.id === id) return { track: t, clip: c };
  return null;
}

/** Notes as command JSON (beats). */
export const notesParam = (song: Song, notes: { id?: number; pitch: number; start: number; length: number; velocity: number }[]) =>
  notes.map((n) => ({ id: n.id || undefined, pitch: n.pitch, start: ticksToBeats(song, n.start), length: ticksToBeats(song, n.length), velocity: n.velocity }));
