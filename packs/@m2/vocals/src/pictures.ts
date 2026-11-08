// @m2/vocals' picture (VOX-27): Tune's pitch line, in TypeScript over the picture words (CARD-61).
// The line is no sum of the pack's: it is what Tune's own kernel hears and aims at, its meters
// (`@m2/vocals:snap`'s `heard` and `note`, VOX-22), which the window draws as they come. A picture
// never reads the song's key: the card says it under its name (VOX-26), and the note line is what
// `snap` aimed at in it.

import { axis, live, view, type Painter, type PictureDecl, type Shape } from "@m2/core";
import { secondsWords, type Values } from "std/pictures";

/** The notes the picture holds, as MIDI numbers: C2 to C6, round the voices `pitch` hears (70 Hz to 1.2 kHz). */
export const LOWEST = 36;
export const HIGHEST = 84;

/** A note's name with its octave, as the roll names it: 60 is "C4". */
const NAMES = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
export const noteName = (midi: number) => `${NAMES[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;

/**
 * Tune's pitch line (VOX-27): its last 3 seconds across, and up the notes from C2 to C6 as MIDI
 * numbers, an axis drawn bare (its Cs name it); the pitch as sung (faint) and the note of the key it
 * moves it to, `snap`'s meters, as they come. With no pitch both are 0, under the axis: they rest at
 * the foot.
 */
export function pitchPicture(): PictureDecl<"seconds", "semitones"> {
  return view.picture({
    x: axis.time(3),
    y: axis.lin("semitones", LOWEST, HIGHEST, { bare: true }),
    live: [live.history(live.meter("heard"), { tone: "faint" }), live.history(live.meter("note"))],
  });
}

/** Middle C, the one C named on the picture: at 48 px tall an octave is 9 px, too little for a name each. */
const MIDDLE_C = 60;

/**
 * The pitch line's guides: a faint line at each C, an octave apart, middle C named just above its
 * own, and now at the right edge, so the axis of time keeps its 3 seconds (CARD-102).
 */
export function pitchLine<D>(p: Painter<D, "seconds", "semitones">): Shape[] {
  const shapes: Shape[] = [p.mark("x", 3, "Now: the last 3 seconds run from the left")];
  for (let c = LOWEST + 12; c < HIGHEST; c += 12) shapes.push(p.mark("y", c, c === MIDDLE_C ? "C4, middle C" : noteName(c)));
  shapes.push(p.label(0.04, MIDDLE_C + 1, noteName(MIDDLE_C)));
  return shapes;
}

/** What the pitch line draws, in words, and how far and how fast Tune moves the voice (Glide, Amount). */
export function pitchWords(v: Values, names: { glide: string; amount: string }): string {
  const [g, a] = [v[names.glide], v[names.amount]];
  const how =
    typeof g !== "number" || typeof a !== "number"
      ? "as its glide and amount, set by a cable or the code, say"
      : a === 0
        ? "Amount 0, so it stays as sung"
        : `${g === 0 ? "there at once" : `in about ${secondsWords(g)}`}, ${a === 1 ? "all the way" : `${Math.round(a * 100)}% of the way`}`;
  return `Your voice (faint), and the note of the key it moves it to: ${how}. Silence and breaths rest at the foot`;
}
