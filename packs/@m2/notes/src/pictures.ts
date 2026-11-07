// @m2/notes' pictures (PACK-30 to PACK-33): what each card draws, in TypeScript over the picture
// words (CARD-61), from the card's values alone (CARD-63). A picture never sees the notes that play,
// nor the song's key, so each is drawn for C: the Arpeggiator's steps for a C major chord, Chord's
// chord on C, Scale's keys in C major. The sums are the kernels' own (packs/@m2/notes/kernels-src):
// the steps' grid, the chords' notes, the echoes' times and levels.

import { axis, live, view, type Painter, type PictureDecl, type Shape, type Unit } from "@m2/core";
import { figures, known, type Values } from "std/pictures";

// ---------------------------------------------------------------- what the pictures share

/** The beats of each note length the menus name: a T is two thirds of its plain one (the kernels' `RATES`). */
export const RATES = { "1/4": 1, "1/8": 0.5, "1/8T": 1 / 3, "1/16": 0.25, "1/16T": 1 / 6, "1/32": 0.125 } as const;
export type Rate = keyof typeof RATES;
/** The menus' note lengths, in their order. */
export const RATE_NAMES = ["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"] as const;
/** A rate in words, for steps: "sixteenth notes". */
const RATE_WORDS: Record<Rate, string> = {
  "1/4": "quarter notes",
  "1/8": "eighth notes",
  "1/8T": "eighth-note triplets",
  "1/16": "sixteenth notes",
  "1/16T": "sixteenth-note triplets",
  "1/32": "thirty-second notes",
};
/** A rate in words, for one: "an eighth note". */
const ONE_WORDS: Record<Rate, string> = {
  "1/4": "a quarter note",
  "1/8": "an eighth note",
  "1/8T": "an eighth-note triplet",
  "1/16": "a sixteenth note",
  "1/16T": "a sixteenth-note triplet",
  "1/32": "a thirty-second note",
};

/** A pitch class's name, as the roll names a key's root: "C", "E♭". */
const NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
/** A note's name with its octave: 60 is "C4". */
export const noteName = (p: number) => `${NAMES[((p % 12) + 12) % 12]}${Math.floor(p / 12) - 1}`;

/** The choice `v` holds by `name` (CARD-63): one a lane sets or the code works out greys the picture, saying which. */
function named(v: Values, name: string, what: string): string {
  const x = v[name];
  if (x === undefined) throw new Error(`names ${name}, which this sound doesn't have`);
  if (typeof x !== "string") throw new Error(`its ${what} is set by a lane or worked out by the code, so it can't be drawn`);
  return x;
}

/** `x` within `lo` and `hi`: a value past its range plays as its nearest end (PACK-29). */
const within = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

/** A box from `x0` to `x1` across and `y0` to `y1` up: its outline, and when `lit`, filled and drawn full. */
function box<D, X extends Unit, Y extends Unit>(p: Painter<D, X, Y>, x0: number, x1: number, y0: number, y1: number, lit: boolean): Shape[] {
  const corners = [x0, y0, x1, y0, x1, y1, x0, y1];
  return [
    ...(lit ? [p.fill(corners, { tone: "main" })] : []),
    p.line([...corners, x0, y0], { tone: lit ? "main" : "faint" }),
  ];
}

// ---------------------------------------------------------------- the Arpeggiator's steps (PACK-30)

/** The C major chord the steps picture is drawn for: C4, E4 and G4. */
const C_CHORD = [60, 64, 67];

/**
 * The steps picture (PACK-30): one turn of the pattern, across in beats from its start (an axis of
 * beats, so the window fits it to the turn, CARD-102), each note's place up, from the lowest to the
 * highest; and the step playing now, a line from the kernel's meter `step`.
 */
export function stepsPicture(): PictureDecl<"beats", "scalar"> {
  return view.picture({ x: axis.lin("beats", 0, 24), y: axis.lin("scalar", 0, 1), live: [live.playhead("step")] });
}

/**
 * One turn of the pattern for a C major chord (PACK-30): each step's note in turn, as the kernel
 * orders them. Up by pitch, each octave up `octaves` times, none past 127; Down backwards; Up and
 * down back down without the top or the bottom again. As played is Up, the chord's notes pressed
 * on one frame; Random draws the notes it picks from, by pitch.
 */
export function turn(style: string, octaves: number): number[] {
  const up: number[] = [];
  for (let o = 0; o < octaves; o++) for (const p of C_CHORD) if (p + 12 * o <= 127) up.push(p + 12 * o);
  if (style === "down") return [...up].reverse();
  if (style === "updown") return [...up, ...up.slice(1, -1).reverse()];
  return up;
}

/** Where step `k` of a turn starts and how long it lasts, in beats (the kernel's `step_of`): the second of each pair swung later. */
export function stepOf(k: number, rate: number, swing: number): [number, number] {
  const pair = Math.floor(k / 2) * (2 * rate);
  return k % 2 === 0 ? [pair, 2 * swing * rate] : [pair + 2 * swing * rate, (2 - 2 * swing) * rate];
}

/** How tall a step's box is, as a share of the picture: half of it above its note's place, half below. */
const STEP_HEIGHT = 0.06;

/**
 * The steps (PACK-30): each step a box from its step's start, `gate` × its step long, at its note's
 * place; each of the chord's notes named at its first box; a guide line where a turn starts (the
 * step's line rests there while no note is held) and one where the next starts.
 */
export function steps<D>(p: Painter<D, "beats", "scalar">, v: Values): Shape[] {
  const style = named(v, "style", "style");
  const rate = RATES[named(v, "rate", "rate") as Rate] ?? RATES["1/16"];
  const octaves = within(Math.round(Number(named(v, "octaves", "octaves"))), 1, 4);
  const gate = within(known(v, "gate", "gate"), 0.1, 2);
  const swing = within(known(v, "swing", "swing"), 0.5, 0.71);
  const notes = turn(style, octaves);
  const [lo, hi] = [Math.min(...notes), Math.max(...notes)];
  const place = (pitch: number) => (hi === lo ? 0.5 : 0.12 + (0.72 * (pitch - lo)) / (hi - lo));
  const shapes: Shape[] = [p.mark("x", 0, "Where a turn starts: the line rests here while no note is held")];
  const named_ = new Set<number>();
  notes.forEach((pitch, k) => {
    const [start, len] = stepOf(k, rate, swing);
    const y = place(pitch);
    shapes.push(...box(p, start, start + gate * len, y - STEP_HEIGHT / 2, y + STEP_HEIGHT / 2, true));
    if (C_CHORD.includes(pitch) && !named_.has(pitch)) {
      named_.add(pitch);
      shapes.push(p.label(start, y + STEP_HEIGHT, noteName(pitch)));
    }
  });
  shapes.push(p.mark("x", stepOf(notes.length, rate, swing)[0], "Then the pattern starts again"));
  return shapes;
}

const STYLE_WORDS: Record<string, string> = { up: "Up", down: "Down", updown: "Up and down", played: "As played", random: "Random" };

/** What `steps` draws, in words: "Up, sixteenth notes, 2 octaves". */
export function stepsWords(v: Values): string {
  const [style, rate, octaves] = [v.style, v.rate, v.octaves];
  if (typeof style !== "string" || typeof rate !== "string" || typeof octaves !== "string") {
    return "Plays the notes you hold one at a time: its style, rate or octaves is set by a lane or the code";
  }
  const n = Number(octaves);
  const held = v.hold === true ? ", held while stopped" : "";
  return `${STYLE_WORDS[style] ?? style}, ${RATE_WORDS[rate as Rate] ?? rate}, ${n} ${n === 1 ? "octave" : "octaves"}${held}`;
}

// ---------------------------------------------------------------- a keyboard (Chord, Scale)

/** The keys of an octave that are black, from C. */
const BLACK = [1, 3, 6, 8, 10];

/**
 * A keyboard of `count` keys from C, across in semitones from it: each key a box, a black one
 * shorter; those `lit` filled, and each key named by `name` (when it gives one) at its foot.
 */
function keyboard<D>(p: Painter<D, "semitones", "unit01">, count: number, lit: (k: number) => boolean, name: (k: number) => string | null): Shape[] {
  const shapes: Shape[] = [];
  for (let k = 0; k < count; k++) {
    const black = BLACK.includes(k % 12);
    shapes.push(...box(p, k + 0.08, k + 0.92, black ? 0.4 : 0.04, 0.96, lit(k)));
    const words = name(k);
    if (words) shapes.push(p.label(k + 0.12, black ? 0.44 : 0.08, words));
  }
  return shapes;
}

/** A keyboard picture of `count` keys from C (PACK-31, PACK-33). */
function keyboardPicture(count: number): PictureDecl<"semitones", "unit01"> {
  return view.picture({ x: axis.lin("semitones", 0, count), y: axis.lin("unit01", 0, 1) });
}

/** C major's notes, from C: the key Chord and Scale draw in. */
const C_MAJOR = [0, 2, 4, 5, 7, 9, 11];
const inC = (p: number) => C_MAJOR.includes(((p % 12) + 12) % 12);
/** The note of C major nearest `p`, a tie going down (NOTE-3), as the kernels snap. */
const snapC = (p: number) => [0, -1, 1, -2, 2].map((d) => p + d).find(inC) ?? p;
/** The note `steps` notes of C major from `from`, one of its notes: up above 0, down below. */
function stepC(from: number, steps: number): number {
  let p = from;
  for (let i = 0; i < Math.abs(steps); i++) {
    p += Math.sign(steps);
    while (!inC(p)) p += Math.sign(steps);
  }
  return p;
}

// ---------------------------------------------------------------- Chord's keyboard (PACK-31)

/** Chord's shapes, in its menu's order. */
export const SHAPES = ["octave", "fifth", "triad", "7th", "sus2", "sus4", "add9"] as const;
/** Each shape in the key's notes from the root (the stamps' counting), an octave as 12 semitones (null). */
const IN_KEY_STEPS: (number | null)[][] = [[0, null], [0, 4], [0, 2, 4], [0, 2, 4, 6], [0, 1, 4], [0, 3, 4], [0, 2, 4, 8]];
/** Each shape's fixed notes, in semitones: a major triad, a dominant 7th. */
const FIXED = [[0, 12], [0, 7], [0, 4, 7], [0, 4, 7, 10], [0, 2, 7], [0, 5, 7], [0, 4, 7, 14]];

/** The notes the chord of `shape` plays for C4 (60): in C major when `inKey`, else fixed; `inversion` times turned over. */
export function chordOnC(shape: number, inKey: boolean, inversion: number): number[] {
  let notes = inKey ? IN_KEY_STEPS[shape].map((s) => (s === null ? 72 : stepC(60, s))) : FIXED[shape].map((o) => 60 + o);
  for (let i = 0; i < inversion; i++) {
    const low = Math.min(...notes);
    const at = notes.indexOf(low);
    notes = notes.map((n, j) => (j === at ? n + 12 : n));
  }
  return [...new Set(notes)].filter((n) => n <= 127).sort((a, b) => a - b);
}

/** Chord's keyboard (PACK-31): two octaves and a note, C4 to C6. */
export function chordPicture(): PictureDecl<"semitones", "unit01"> {
  return keyboardPicture(25);
}

/** The notes the chord plays for a C, lit and named (PACK-31). */
export function chordKeys<D>(p: Painter<D, "semitones", "unit01">, v: Values): Shape[] {
  const shape = SHAPES.indexOf(named(v, "shape", "shape") as (typeof SHAPES)[number]);
  const inKey = v.inKey;
  if (typeof inKey !== "boolean") throw new Error("its In key is set by a lane or worked out by the code, so it can't be drawn");
  const notes = chordOnC(Math.max(0, shape), inKey, within(Math.round(known(v, "inversion", "inversion")), 0, 3));
  return keyboard(p, 25, (k) => notes.includes(60 + k), (k) => (notes.includes(60 + k) ? NAMES[k % 12] : null));
}

/** What a shape is called, in a sentence: in a key, and fixed. */
const SHAPE_WORDS: Record<string, [string, string]> = {
  octave: ["An octave", "An octave"],
  fifth: ["A fifth of the key", "A fifth"],
  triad: ["A triad of the key", "A major triad"],
  "7th": ["A 7th of the key", "A dominant 7th"],
  sus2: ["A sus2 of the key", "A sus2"],
  sus4: ["A sus4 of the key", "A sus4"],
  add9: ["An add9 of the key", "An add9"],
};

/** What `chordKeys` draws, in words: "A triad of the key on each note: on C in C major, C E G". */
export function chordWords(v: Values): string {
  const [shape, inKey, inversion] = [v.shape, v.inKey, v.inversion];
  if (typeof shape !== "string" || typeof inKey !== "boolean" || typeof inversion !== "number") {
    return "Plays a chord from each note: its shape, In key or inversion is set by a lane or the code";
  }
  const turned = within(Math.round(inversion), 0, 3);
  const notes = chordOnC(Math.max(0, SHAPES.indexOf(shape as (typeof SHAPES)[number])), inKey, turned);
  const [keyed, fixed] = SHAPE_WORDS[shape] ?? [shape, shape];
  const on = inKey ? "on C in C major" : "on C";
  return `${inKey ? keyed : fixed} on each note: ${on}, ${notes.map((n) => NAMES[n % 12]).join(" ")}${turned ? `, inversion ${turned}` : ""}`;
}

// ---------------------------------------------------------------- Note echo's echoes (PACK-32)

/** Note echo's echoes (PACK-32): beats across from the note's start (fitted to them, CARD-102), how hard each plays up. */
export function echoesPicture(): PictureDecl<"beats", "unit01"> {
  return view.picture({ x: axis.lin("beats", 0, 9), y: axis.lin("unit01", 0, 1) });
}

/** The note and each echo: where it starts, in beats, how hard it plays (the note at 1), and how far it moves. None silent or past 127. */
export function echoLevels(every: number, repeats: number, fall: number, pitch: number): { at: number; level: number; moves: number }[] {
  const out = [{ at: 0, level: 1, moves: 0 }];
  for (let k = 1; k <= repeats; k++) {
    const level = (1 - fall) ** k;
    if (level > 0) out.push({ at: k * every, level, moves: k * pitch });
  }
  return out;
}

/** The echoes as fading bars, each named by how far it moves when `pitch` isn't 0 (PACK-32). */
export function echoBars<D>(p: Painter<D, "beats", "unit01">, v: Values): Shape[] {
  const every = RATES[named(v, "time", "time") as Rate] ?? RATES["1/8"];
  const repeats = within(Math.round(known(v, "repeats", "repeats")), 1, 8);
  const fall = within(known(v, "fall", "fall"), 0, 1);
  const pitch = within(Math.round(known(v, "pitch", "pitch")), -12, 12);
  const shapes: Shape[] = [];
  for (const e of echoLevels(every, repeats, fall, pitch)) {
    shapes.push(...box(p, e.at, e.at + every / 2, 0, e.level, true));
    if (pitch !== 0 && e.at > 0) shapes.push(p.label(e.at, Math.min(0.92, e.level + 0.04), `${e.moves > 0 ? "+" : ""}${figures(e.moves)}`));
  }
  return shapes;
}

/** What `echoBars` draws, in words: "3 echoes, an eighth note apart, each 30% quieter". */
export function echoWords(v: Values): string {
  const [time, repeats, fall, pitch] = [v.time, v.repeats, v.fall, v.pitch];
  if (typeof time !== "string" || typeof repeats !== "number" || typeof fall !== "number" || typeof pitch !== "number") {
    return "Plays each note again, later and quieter: one of its values is set by a lane or the code";
  }
  const n = within(Math.round(repeats), 1, 8);
  const quieter = within(fall, 0, 1) > 0 ? `each ${Math.round(within(fall, 0, 1) * 100)}% quieter` : "each as loud";
  const by = within(Math.round(pitch), -12, 12);
  const climbs = by === 0 ? "" : `, each ${Math.abs(by)} ${Math.abs(by) === 1 ? "semitone" : "semitones"} ${by > 0 ? "higher" : "lower"}`;
  return `${n} ${n === 1 ? "echo" : "echoes"}, ${ONE_WORDS[time as Rate] ?? time} apart, ${quieter}${climbs}`;
}

// ---------------------------------------------------------------- Scale's keyboard (PACK-33)

/** Scale's keyboard (PACK-33): the twelve notes from C. */
export function scalePicture(): PictureDecl<"semitones", "unit01"> {
  return keyboardPicture(12);
}

/** The twelve notes from C, C major's lit, each named by the note it plays in C major at this shift (PACK-33). */
export function scaleKeys<D>(p: Painter<D, "semitones", "unit01">, v: Values): Shape[] {
  const shift = within(Math.round(known(v, "shift", "shift")), -7, 7);
  return keyboard(p, 12, inC, (k) => NAMES[((stepC(snapC(60 + k), shift) % 12) + 12) % 12]);
}

/** What `scaleKeys` draws, in words. */
export function scaleWords(v: Values): string {
  const shift = v.shift;
  const near = "Moves each note to the nearest note of the key, a tie going down";
  if (typeof shift !== "number") return `${near}: its shift is set by a lane or the code`;
  const s = within(Math.round(shift), -7, 7);
  const then = s === 0 ? "" : `, then ${Math.abs(s)} ${Math.abs(s) === 1 ? "note" : "notes"} of the key ${s > 0 ? "up" : "down"}`;
  return `${near}${then}. Drawn in C major: the card says your song's key`;
}
