// @m2/vocals: tools for a sung voice (PACK-15). Tune moves each sung note onto the song's key.
//
// It is three of the pack's own kernels in a row (VOX-20; built from kernels-src/, PACK-21):
// `pitch` finds the voice's pitch and how sure it is, `snap` works out how many semitones move it
// to the nearest note of the key (`time.key`, VOX-12), and `shift` moves it, keeping the voice's
// own colour. `shift` is 15 ms late: the engine delays the rest of the song to match (RENDER-18).
// With no key set, every note is allowed, and the card says so (VOX-26).
//
// Its card has two knobs. Glide is how long a note takes to reach its pitch: 0 is Robot, the
// stepped pop sound; 0.1 s, Natural, where it opens, keeps the singer's vibrato. Amount is how
// much of each note's error is fixed. Its presets are Natural and Robot.

import { component, control, kernel, port, signature, time, type Audio, type Params } from "@m2/core";

/** Tune: each sung note of `x` moved onto the nearest note of the song's key. */
export const tune = component(
  {
    id: "tune",
    label: "Tune",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      glide: control.secs(0.1).range(0, 0.4),
      amount: control.frac(1).range(0, 1),
    },
    output: port.audio(),
    editor: {
      category: "Voice",
      description: "Moves each sung note onto the song's key: Glide 0 steps like a robot, more slides naturally",
      insert: { glide: 0.1, amount: 1 },
    },
  },
  (x: Audio, { glide, amount }: Params) => {
    const sung = kernel("@m2/vocals:pitch", { in: x });
    const hz = sung.port("hz");
    const clarity = sung.port("clarity");
    const by = kernel("@m2/vocals:snap", { hz, clarity, key: time.key, glide, amount });
    return kernel("@m2/vocals:shift", { in: x, semitones: by, hz, clarity });
  },
);
