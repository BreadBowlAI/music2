// The piano roll's rows, by name (polish A4, Q7): a pitch's name ("C2"), or what the instrument
// that plays the lane calls the note ("Kick", "Snare", "Closed hat"), which the timeline gives a
// notes lane as `names` (by MIDI note) when its instrument names its notes (a drum kit).

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** A lane as far as its names go: the timeline's notes lane (other kinds have none). */
export type Named = { key: string; names?: Record<string, string> } | null | undefined;

/** A pitch's name: 60 is C4. */
export const pitchName = (p: number) => `${NOTE_NAMES[((p % 12) + 12) % 12]}${Math.floor(p / 12) - 1}`;

const NONE: Record<string, string> = {};

/** The lane's note names, by MIDI note: empty for most lanes (always the same object, so a memo holds). */
export const namesOf = (lane: Named): Record<string, string> => lane?.names ?? NONE;

/** Whether the lane's notes have names: then its keys say them, and the key column is wider. */
export const named = (lane: Named) => Object.keys(namesOf(lane)).length > 0;

/** What row `pitch` is called on `lane`: its note's name ("Kick"), else the pitch's ("C2"). */
export const rowName = (lane: Named, pitch: number) => namesOf(lane)[pitch] ?? pitchName(pitch);

/** The key column's width: 88 px where the keys say names, else 56 (Q7). */
export const keysWidth = (lane: Named) => (named(lane) ? 88 : 56);
