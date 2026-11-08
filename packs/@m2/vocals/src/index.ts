// @m2/vocals: tools for a sung voice (PACK-15). Tune moves each sung note onto the song's key.
//
// It is three of the pack's own kernels in a row (VOX-20; built from kernels-src/, PACK-21):
// `pitch` finds the voice's pitch and how sure it is, `snap` works out how many semitones move it
// to the nearest note of the key (`time.key`, VOX-12), and `shift` moves it, keeping the voice's
// own colour. `shift` is 15 ms late: the engine delays the rest of the song to match (RENDER-18).
// With no key set, every note is allowed, and the card says so (VOX-26).
//
// Its card has three knobs. Glide is how long a note takes to reach its pitch: 0 is Robot, the
// stepped pop sound; 0.1 s, Natural, where it opens, keeps the singer's vibrato. Amount is how
// much of each note's error is fixed. Formant moves the voice's colour and not its notes, as it is
// to shift's `formant`: at 0, Tune plays what it played before it had the knob. Its presets are
// Natural and Robot. Its face draws its pitch line (VOX-27, ./pictures): how far the voice is from
// its note, and how far Tune leaves it, from snap's meters.
//
// The Voice changer (VOX-28) is two of the same kernels: pitch, and shift moving every note by
// Pitch and the colour by Formant, with no snap, so it follows no key. Its face draws its spectrum
// as the voice comes in and goes out, live. Its presets are Deep and Chipmunk.

import { component, control, kernel, port, signature, time, type Audio, type Params } from "@m2/core";
import { pitchLine, pitchPicture, pitchWords, voiceNames, voicePicture, voiceWords } from "./pictures";

/** Tune: each sung note of `x` moved onto the nearest note of the song's key; `formant` (semitones) moves its colour on its own. */
export const tune = component(
  {
    id: "tune",
    label: "Tune",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      glide: control.secs(0.1).range(0, 0.4),
      amount: control.frac(1).range(0, 1),
      // Its colour, not its notes: down deeper and darker, up smaller and brighter (VOX-20).
      formant: control.semis(0).range(-12, 12),
    },
    output: port.audio(),
    editor: {
      category: "Voice",
      description: "Moves each sung note onto the song's key: Glide 0 steps like a robot, more slides naturally",
      insert: { glide: 0.1, amount: 1 },
      // Its pitch line: how far the voice is from its note, and how far Tune leaves it, as they come (VOX-27).
      views: { pitch: pitchPicture() },
      face: ["pitch", "glide", "amount", "formant"],
    },
  },
  (x: Audio, { glide, amount, formant }: Params) => {
    const sung = kernel("@m2/vocals:pitch", { in: x });
    const hz = sung.port("hz");
    const clarity = sung.port("clarity");
    const by = kernel("@m2/vocals:snap", { hz, clarity, key: time.key, glide, amount });
    return kernel("@m2/vocals:shift", { in: x, semitones: by, hz, clarity, formant });
  },
  { pitch: { draw: (_v, p) => pitchLine(p), words: (v) => pitchWords(v, { glide: "glide", amount: "amount" }) } },
);

/**
 * Voice changer (VOX-28): the notes of the voice `x` moved by `pitch` semitones, and its colour by
 * `formant` semitones, each on its own: both down for a deep voice, both up for a chipmunk. It is
 * `pitch` and `shift`, with no `snap`, so it follows no key; 15 ms late, as Tune is.
 */
export const voiceChanger = component(
  {
    id: "voice-changer",
    label: "Voice changer",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      // As far as shift reaches (VOX-23).
      pitch: control.semis(0).range(-7, 12),
      formant: control.semis(0).range(-12, 12),
    },
    output: port.audio(),
    editor: {
      category: "Voice",
      description: "Pitch moves the notes, Formant the colour: both down for a deep voice, both up for a chipmunk",
      insert: { pitch: 0, formant: 0 },
      // Its spectrum as the voice comes in and goes out, live: nothing worked out from sums.
      views: { voice: voicePicture() },
      face: ["voice", "pitch", "formant"],
    },
  },
  (x: Audio, { pitch, formant }: Params) => {
    const sung = kernel("@m2/vocals:pitch", { in: x });
    return kernel("@m2/vocals:shift", { in: x, semitones: pitch, hz: sung.port("hz"), clarity: sung.port("clarity"), formant });
  },
  { voice: { draw: (_v, p) => voiceNames(p), words: (v) => voiceWords(v, { pitch: "pitch", formant: "formant" }) } },
);
