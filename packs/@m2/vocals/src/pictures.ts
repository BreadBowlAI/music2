// @m2/vocals' pictures: Tune's pitch line (VOX-27) and the Voice changer's spectrum (VOX-28), in
// TypeScript over the picture words (CARD-61).
// The lines are no sums of the pack's: they are what Tune's own kernel hears and does, its meters
// (`@m2/vocals:snap`'s `off` and `left`, VOX-22), which the window draws as they come. A picture
// never reads the song's key: the card says it under its name (VOX-26), and the meters are what
// `snap` aimed at in it.

import { axis, live, view, type Painter, type PictureDecl, type Shape } from "@m2/core";
import { figures, secondsWords, type Values } from "std/pictures";

/** How far from its note the picture holds, each way, in semitones: further stays at the edge. */
const EDGE = 1;

/**
 * Tune's pitch line (VOX-27): its last 3 seconds across and, up, how far the voice is from its note,
 * from a semitone flat at the foot to a semitone sharp at the top, an axis drawn bare (its middle
 * line names it); how far the voice is from the note `snap` aims at (faint) and how far Tune leaves
 * it, `snap`'s meters, as they come. With no voice both are 0: they rest on the middle line.
 */
export function pitchPicture(): PictureDecl<"seconds", "semitones"> {
  return view.picture({
    x: axis.time(3),
    y: axis.lin("semitones", -EDGE, EDGE, { bare: true }),
    live: [live.history(live.meter("off"), { tone: "faint" }), live.history(live.meter("left"))],
  });
}

/** The middle line's name, on the picture and in its words. */
const ON = "On the note";

/**
 * The pitch line's guides: its middle line, on the note, named just over it at the left, and now
 * at the right edge, so the axis of time keeps its 3 seconds (CARD-102).
 */
export function pitchLine<D>(p: Painter<D, "seconds", "semitones">): Shape[] {
  return [p.mark("x", 3, "Now: the last 3 seconds run from the left"), p.mark("y", 0, ON), p.label(0.04, 0.06 * EDGE, ON)];
}

/** What the pitch line draws, in words, and how far and how fast Tune moves the voice (Glide, Amount). */
export function pitchWords(v: Values, names: { glide: string; amount: string }): string {
  const [g, a] = [v[names.glide], v[names.amount]];
  const when = (g: number) => (g === 0 ? "at once" : `in about ${secondsWords(g)}`);
  const how =
    typeof g !== "number" || typeof a !== "number"
      ? "moves it as its glide and amount, set by a cable or the code, say"
      : a === 0
        ? "leaves it as sung at Amount 0, so the two lines are one"
        : a === 1
          ? `moves it onto the note ${when(g)}${g === 0 ? ", so the solid line stays on the middle line" : ""}`
          : `moves it ${Math.round(a * 100)}% of the way to the note ${when(g)}`;
  return (
    `Faint: how far your voice is from its note. Solid: how far it is after Tune, which ${how}. ` +
    "Up is sharp, down is flat, up to a semitone (further stays at the edge). With no voice (silence, a breath, an S), both rest on the middle line"
  );
}

// ---------------------------------------------------------------- the Voice changer

/**
 * The Voice changer's spectrum (VOX-28): live, as the voice comes in (faint) and goes out, across 20
 * Hz to 20 kHz on a log scale, up −90 to 0 dB, the spectrum's own scale (CARD-103): its notes'
 * harmonics are the spikes that Pitch moves, its vowel's hills the colour that Formant moves. It
 * draws no shapes, nothing worked out from sums: at silence it is still (CARD-73).
 */
export function voicePicture(): PictureDecl<"hz", "db"> {
  return view.picture({
    x: axis.log("hz", 20, 20000),
    y: axis.lin("db", -90, 0),
    live: [live.spectrum("in", { tone: "faint" }), live.spectrum("out")],
  });
}

/** A number of semitones in words: "1 semitone", "2.5 semitones". */
const semitones = (n: number) => `${figures(Math.abs(n))} semitone${Math.abs(n) === 1 ? "" : "s"}`;

/** What the Voice changer's knobs did, in words, then what its two layers are (VOX-28). */
export function voiceWords(v: Values, names: { pitch: string; formant: string }): string {
  const [pitch, formant] = [v[names.pitch], v[names.formant]];
  const layers = "Faint: your voice as it comes in; solid: as it goes out. The spikes are its notes, the hills its colour";
  if (pitch === 0 && formant === 0) return `As sung: Pitch moves the notes, Formant the colour (both down for a deep voice, both up for a chipmunk). ${layers}`;
  const notes = typeof pitch !== "number" ? "Notes moved by a cable or the code" : pitch === 0 ? "Notes as sung" : `${pitch > 0 ? "Up" : "Down"} ${semitones(pitch)}`;
  const colour =
    typeof formant !== "number" ? "colour moved by a cable or the code" : formant === 0 ? "colour as sung" : `colour ${semitones(formant)} ${formant > 0 ? "brighter" : "darker"}`;
  return `${notes}, ${colour}. ${layers}`;
}
