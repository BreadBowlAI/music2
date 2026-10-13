// std/fx: effects.
//
// Each effect declares an `editor.category`, so the editor offers it after any audio and
// writes its `insert` when it is added. A control is `.smooth()` when its kernel reads it
// every sample without smoothing it, so a step would click (PKG-14); the others' kernels
// smooth their controls themselves, or read them once per block.

import { component, control, kernel, port, signature, type Audio, type Params, type Value } from "@m2/core";

/** Turn `x` down by `depth` dB while `side` is high. Use another track's `follow()` export as `side`. */
export const duck = component(
  {
    id: "duck",
    label: "Duck",
    signature: signature<(x: Audio, side: Value) => Audio>(),
    // `side` only listens: muting the track it comes from must not stop the ducking (PKG-13).
    inputs: { in: port.audio(), side: port.control("unit01", { sidechain: true }) },
    controls: { depth: control.db(-8).range(-60, 0) },
    output: port.audio(),
  },
  (x: Audio, side: Value, { depth }: Params) => kernel("std:duck", { in: x, side, depth }),
);

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

/** Peak limiter with a hard ceiling in dB. */
export const limit = component(
  {
    id: "limit",
    label: "Limit",
    signature: signature<(x: Audio, ceiling?: Value) => Audio>(),
    inputs: { in: port.audio() },
    // Not smooth: the kernel reads it once per block, which a one-block ramp can't reach.
    controls: { ceiling: control.db(-1).range(-24, 0) },
    output: port.audio(),
    positional: ["ceiling"],
    editor: { category: "Dynamics", description: "Stops peaks from clipping", insert: { ceiling: -1 } },
  },
  (x: Audio, { ceiling }: Params) => kernel("std:limit", { in: x, ceiling }),
);

// ---------------------------------------------------------------- space

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
  (x: Audio, { rate, depth, mix }: Params) => kernel("std:chorus", { in: x, rate, depth, mix }),
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
  (x: Audio, { rate, depth, feedback, mix }: Params) => kernel("std:phaser", { in: x, rate, depth, feedback, mix }),
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
  (x: Audio, { rate, depth }: Params) => kernel("std:tremolo", { in: x, rate, depth }),
);

// ---------------------------------------------------------------- dynamics and tone

/**
 * Compressor: above `threshold` (dB) the level rises only 1/`ratio` as fast, so loud and
 * quiet moments even out. `attack` and `release` (seconds) are how fast it grabs and lets
 * go; `makeup` (dB) brings the level back up.
 */
export const compress = component(
  {
    id: "compress",
    label: "Compress",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      threshold: control.db(-18).range(-60, 0),
      ratio: control.scalar(3).range(1, 20).log(),
      attack: control.secs(0.01).range(0.0001, 1).log(),
      release: control.secs(0.15).range(0.001, 5).log(),
      makeup: control.db(0).range(0, 24),
    },
    output: port.audio(),
    // Added with some makeup gain, though left out it is 0 dB.
    editor: {
      category: "Dynamics",
      description: "Evens out loud and quiet moments so the sound sits steady",
      insert: { threshold: -18, ratio: 3, attack: 0.01, release: 0.15, makeup: 4 },
    },
  },
  (x: Audio, { threshold, ratio, attack, release, makeup }: Params) => kernel("std:compress", { in: x, threshold, ratio, attack, release, makeup }),
);

/** Three-band EQ: `low`, `mid` and `high` in dB (0 leaves the sound alone), around `lowFreq`, `midFreq` and `highFreq` in Hz. */
export const eq3 = component(
  {
    id: "eq3",
    label: "EQ3",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      low: control.db(0).range(-60, 24),
      mid: control.db(0).range(-60, 24),
      high: control.db(0).range(-60, 24),
      lowFreq: control.hz(200).range(20, 20000).log(),
      midFreq: control.hz(1000).range(20, 20000).log(),
      highFreq: control.hz(4000).range(20, 20000).log(),
    },
    output: port.audio(),
    editor: { category: "EQ", description: "Bass, middle and treble, each up or down in dB", insert: { low: 0, mid: 0, high: 0 } },
  },
  (x: Audio, { low, mid, high, lowFreq, midFreq, highFreq }: Params) =>
    kernel("std:eq3", { in: x, low, mid, high, low_freq: lowFreq, mid_freq: midFreq, high_freq: highFreq }),
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
  (x: Audio, { bits, rate, mix }: Params) => kernel("std:bitcrush", { in: x, bits, rate, mix }),
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
