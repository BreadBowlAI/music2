// @m2/notes: note effects (PACK-29), sounds that take notes and give notes, before an instrument:
// keys → Arpeggiator → Chord → Pluck. Each is the pack's own kernel (built from kernels-src/,
// PACK-21) that gives notes (ABI-15), which music2 runs in the lane pass, right after the lane it
// reads, behind its guard (RENDER-41, RENDER-42).
//
// A note effect is a function from notes to notes, so a song's code reads inside out:
// `pluck(arp(lane.notes("chords"), { rate: "1/16" }))`. + Note effect, or `m2 add-effect`, writes
// the same: it wraps the instrument's notes (PKG-21).
//
// Each card's face is its picture, drawn for C (a picture sees only the card's values, CARD-63),
// and every value but the Arpeggiator's swing. Chord and Scale read the song's key, `time.key`, so
// their cards say it, and Set the key when there is none (VOX-26).

import { component, control, kernel, port, setting, time, type Events, type EventsKind, type Params } from "@m2/core";
import {
  chordKeys,
  chordPicture,
  chordWords,
  echoBars,
  echoesPicture,
  echoWords,
  RATE_NAMES,
  scaleKeys,
  scalePicture,
  scaleWords,
  SHAPES,
  steps,
  stepsPicture,
  stepsWords,
} from "./pictures";

/**
 * The Arpeggiator (PACK-30): the notes you hold, played one at a time, in a pattern, over and over,
 * on the song's grid while it plays and at its tempo from the first key while it is stopped.
 * `style` up, down, updown (up and down), played (as played) or random; `rate` the length of a step
 * (a T is a triplet); `octaves` how many octaves the pattern climbs; `gate` how long each step
 * sounds, as a share of the step (over 1 overlaps the next); `hold` keeps playing the keys you let
 * go while the song is stopped; `swing` moves every second step later. Random's choices come from
 * the setting `seed`.
 */
export const arp = component(
  {
    id: "arp",
    label: "Arpeggiator",
    inputs: { notes: port.notes() },
    controls: {
      style: control.choice(["up", "down", "updown", "played", "random"], "up"),
      rate: control.choice(RATE_NAMES, "1/16"),
      octaves: control.choice(["1", "2", "3", "4"], "1"),
      gate: control.scalar(0.8).range(0.1, 2),
      hold: control.bool(false),
      swing: control.frac(0.5).range(0.5, 0.71),
    },
    settings: { seed: setting.int(1) },
    output: port.notes(),
    editor: {
      category: "Arpeggiator",
      description: "Plays the notes you hold one at a time, in a pattern, in time with the song",
      // The pattern for a C major chord, its step lit by the kernel's meter `step` (PACK-30).
      views: { steps: stepsPicture() },
      face: ["steps", "style", "rate", "octaves", "gate", "hold"],
    },
  },
  (notes: Events, { style, rate, octaves, gate, hold, swing, seed }: Params) =>
    kernel<EventsKind>(
      "@m2/notes:arp",
      { notes, beats: time.beats, playing: time.playing, tempo: time.tempo, style, rate, octaves, gate, hold, swing },
      { values: [seed] },
    ),
  { steps: { draw: (v, p) => steps(p, v), words: (v) => stepsWords(v) } },
);

/**
 * Chord (PACK-31): each note you play becomes a chord. `shape` octave, fifth, triad, 7th, sus2,
 * sus4 or add9; with `inKey` on (and the song has a key), built from the key's own notes, as the
 * roll's chord stamps are: one finger on D in C major plays D minor. Else the shapes are fixed (a
 * major triad, a dominant 7th). `inversion` 0 to 3 moves its lowest notes up an octave.
 */
export const chord = component(
  {
    id: "chord",
    label: "Chord",
    inputs: { notes: port.notes() },
    controls: {
      shape: control.choice(SHAPES, "triad"),
      inKey: control.bool(true).label("In key"),
      inversion: control.scalar(0).range(0, 3),
    },
    output: port.notes(),
    editor: {
      category: "Chords",
      description: "Plays a whole chord from each note: one finger plays the chords of the key",
      // The chord on C, lit on a small keyboard (PACK-31).
      views: { keyboard: chordPicture() },
      face: ["keyboard", "shape", "inKey", "inversion"],
    },
  },
  (notes: Events, { shape, inKey, inversion }: Params) =>
    kernel<EventsKind>("@m2/notes:chord", { notes, key: time.key, shape, in_key: inKey, inversion }),
  { keyboard: { draw: (v, p) => chordKeys(p, v), words: (v) => chordWords(v) } },
);

/**
 * Note echo (PACK-32): each note played again `repeats` times, `time` apart on the song's beat,
 * each `fall` quieter than the last and `pitch` semitones higher (+12 climbs by octaves). Stop and
 * a seek end the echoes.
 */
export const echo = component(
  {
    id: "echo",
    label: "Note echo",
    inputs: { notes: port.notes() },
    controls: {
      time: control.choice(RATE_NAMES, "1/8"),
      repeats: control.scalar(3).range(1, 8),
      fall: control.frac(0.3).range(0, 1),
      pitch: control.semis(0).range(-12, 12),
    },
    output: port.notes(),
    editor: {
      category: "Echo",
      description: "Plays each note again, later and quieter, a few times: a delay of notes",
      // The note and its echoes as fading bars (PACK-32).
      views: { echoes: echoesPicture() },
      face: ["echoes", "time", "repeats", "fall", "pitch"],
    },
  },
  (notes: Events, { time: every, repeats, fall, pitch }: Params) =>
    kernel<EventsKind>("@m2/notes:echo", { notes, beats: time.beats, playing: time.playing, tempo: time.tempo, time: every, repeats, fall, pitch }),
  { echoes: { draw: (v, p) => echoBars(p, v), words: (v) => echoWords(v) } },
);

/**
 * Scale (PACK-33): every note moved to the nearest note of the song's key, a tie going down, as
 * Lock to key does in the roll, so no note is wrong; then `shift` notes of the key up or down (+2
 * in C major turns C E G into E G B). With no key set, notes play as they are.
 */
export const scale = component(
  {
    id: "scale",
    label: "Scale",
    inputs: { notes: port.notes() },
    controls: { shift: control.scalar(0).range(-7, 7) },
    output: port.notes(),
    editor: {
      category: "Scales",
      description: "Moves every note onto the song's key, so no note is wrong",
      // The twelve notes, C major's lit, each named by the note it plays (PACK-33).
      views: { keyboard: scalePicture() },
      face: ["keyboard", "shift"],
    },
  },
  (notes: Events, { shift }: Params) => kernel<EventsKind>("@m2/notes:scale", { notes, key: time.key, shift }),
  { keyboard: { draw: (v, p) => scaleKeys(p, v), words: (v) => scaleWords(v) } },
);
