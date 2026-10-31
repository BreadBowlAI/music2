// @m2/effects: effects that colour a sound (PACK-15): echoes and space, modulation, drive and crush,
// and width. The effects mixing needs (EQ, compression, limiting, ducking) are std's, in std/fx.
//
// Each declares an `editor.category`, so the editor offers it after any audio and writes its
// `insert` when it is added. A control is `.smooth()` when its kernel reads it every sample without
// smoothing it, so a step would click (PKG-14); the others' kernels smooth their controls
// themselves, or read them once per block. Each plays std's kernel of its name, but the
// bitcrusher, which plays the pack's own (`@m2/effects:bitcrush`, built from kernels-src/, PACK-21).
//
// Chorus, phaser, tremolo and bitcrush follow the song: each passes `time.playing` to its
// kernel, so its wobble (or the bitcrusher's sample clock) moves only while the song plays.
// Stopped, they wait; Play carries on. Played after a stop, a song sounds as its export does.

import { component, control, kernel, port, signature, time, type Audio, type Params, type Value } from "@m2/core";

// ---------------------------------------------------------------- time

/** Feedback delay. `time` up to 2 seconds; `mix` 0 is dry, 1 is only echoes. */
export const delay = component(
  {
    id: "delay",
    label: "Delay",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      time: control.secs(0.25).range(0.001, 2).log().smooth(),
      feedback: control.frac(0.3).range(0, 0.95).smooth(),
      mix: control.frac(0.3).range(0, 1).smooth(),
    },
    output: port.audio(),
    editor: { category: "Delay & Reverb", description: "Echoes that repeat and fade away", insert: { time: 0.25, feedback: 0.3, mix: 0.3 } },
  },
  (x: Audio, { time, feedback, mix }: Params) => kernel("std:delay", { in: x, time, feedback, mix }),
);

/**
 * Reverb: the sound of a room around `x`. `size` 0..1 goes from a small room to a huge
 * hall, `damping` 0..1 from a bright tail to a dark one, and `mix` 0..1 from dry to only
 * reverb. A stereo sound gets a stereo tail; a mono one stays mono, so it can still be panned.
 */
export const reverb = component(
  {
    id: "reverb",
    label: "Reverb",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      size: control.frac(0.6).range(0, 1),
      damping: control.frac(0.5).range(0, 1),
      mix: control.frac(0.25).range(0, 1),
    },
    output: port.audio(),
    editor: { category: "Delay & Reverb", description: "Space: the sound of a room or a hall around it", insert: { size: 0.6, damping: 0.5, mix: 0.25 } },
  },
  (x: Audio, { size, damping, mix }: Params) => kernel("std:reverb", { in: x, size, damping, mix }),
);

// ---------------------------------------------------------------- modulation

/** Chorus: thickens `x` with slowly drifting copies (spread left and right if `x` is stereo). `rate` in Hz; `depth` and `mix` 0..1. */
export const chorus = component(
  {
    id: "chorus",
    label: "Chorus",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      rate: control.hz(0.8).range(0.05, 20).log(),
      depth: control.frac(0.5).range(0, 1),
      mix: control.frac(0.5).range(0, 1),
    },
    output: port.audio(),
    editor: { category: "Modulation", description: "Thicker and wider, with a gentle shimmer", insert: { rate: 0.8, depth: 0.5, mix: 0.5 } },
  },
  (x: Audio, { rate, depth, mix }: Params) => kernel("std:chorus", { in: x, rate, depth, mix, playing: time.playing }),
);

/** Phaser: sweeping notches. `rate` in Hz; `depth`, `feedback` (a sharper sound) and `mix` 0..1, where 0.5 sweeps deepest. */
export const phaser = component(
  {
    id: "phaser",
    label: "Phaser",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      rate: control.hz(0.4).range(0.05, 20).log(),
      depth: control.frac(0.7).range(0, 1),
      feedback: control.frac(0.5).range(0, 1),
      mix: control.frac(0.5).range(0, 1),
    },
    output: port.audio(),
    editor: { category: "Modulation", description: "A swirling, sweeping whoosh", insert: { rate: 0.4, depth: 0.7, feedback: 0.5, mix: 0.5 } },
  },
  (x: Audio, { rate, depth, feedback, mix }: Params) => kernel("std:phaser", { in: x, rate, depth, feedback, mix, playing: time.playing }),
);

/** Tremolo: the volume pulses `rate` times a second, dipping by `depth` (1 is down to silence). */
export const tremolo = component(
  {
    id: "tremolo",
    label: "Tremolo",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { rate: control.hz(5).range(0.05, 20).log(), depth: control.frac(0.5).range(0, 1) },
    output: port.audio(),
    editor: { category: "Modulation", description: "Pulses the volume up and down", insert: { rate: 5, depth: 0.5 } },
  },
  (x: Audio, { rate, depth }: Params) => kernel("std:tremolo", { in: x, rate, depth, playing: time.playing }),
);

// ---------------------------------------------------------------- colour

/** Soft saturation; `amount` is dB of drive. */
export const drive = component(
  {
    id: "drive",
    label: "Drive",
    signature: signature<(x: Audio, amount?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { amount: control.db(6).range(0, 36).smooth() },
    output: port.audio(),
    positional: ["amount"],
    editor: { category: "Distortion", description: "Warm saturation, from gentle grit to crunch", insert: { amount: 6 } },
  },
  (x: Audio, { amount }: Params) => kernel("std:drive", { in: x, amount }),
);

/** Bitcrusher: `bits` of resolution (16 is clean, 4 is harsh) at a lower sample `rate` in Hz, like an old sampler. `mix` 0..1. */
export const bitcrush = component(
  {
    id: "bitcrush",
    label: "Bitcrush",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      bits: control.scalar(8).range(1, 16),
      rate: control.hz(12000).range(100, 48000).log(),
      mix: control.frac(1).range(0, 1),
    },
    output: port.audio(),
    editor: { category: "Distortion", description: "Lo-fi digital crunch, like an old sampler", insert: { bits: 8, rate: 12000, mix: 1 } },
  },
  (x: Audio, { bits, rate, mix }: Params) => kernel("@m2/effects:bitcrush", { in: x, bits, rate, mix, playing: time.playing }),
);

// ---------------------------------------------------------------- stereo

/** Stereo width: 0 is mono, 1 leaves it alone, 2 is twice as wide. The result is stereo: a mono sound gets width too, and still sums cleanly back to mono. */
export const widen = component(
  {
    id: "widen",
    label: "Widen",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { width: control.scalar(1.5).range(0, 2) },
    output: port.audio(),
    editor: { category: "Utility", description: "Makes the stereo image wider, or narrower toward mono", insert: { width: 1.5 } },
  },
  (x: Audio, { width }: Params) => kernel("std:widen", { in: x, width }),
);
