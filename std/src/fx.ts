// std/fx: the effects mixing needs (PKG-17): EQ, compression, a limiter and ducking. The effects
// that colour a sound (delay, reverb, chorus, drive …) are the pack @m2/effects.
//
// Each listed effect declares an `editor.category`, so the editor offers it after any audio and
// writes its `insert` when it is added. A control is `.smooth()` when its kernel reads it every
// sample without smoothing it, so a step would click (PKG-14); the others' kernels smooth their
// controls themselves, or read them once per block.

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
      lowFreq: control.hz(200).range(20, 20000).log().label("low frequency"),
      midFreq: control.hz(1000).range(20, 20000).log().label("mid frequency"),
      highFreq: control.hz(4000).range(20, 20000).log().label("high frequency"),
    },
    output: port.audio(),
    editor: { category: "EQ", description: "Bass, middle and treble, each up or down in dB", insert: { low: 0, mid: 0, high: 0 } },
  },
  (x: Audio, { low, mid, high, lowFreq, midFreq, highFreq }: Params) =>
    kernel("std:eq3", { in: x, low, mid, high, low_freq: lowFreq, mid_freq: midFreq, high_freq: highFreq }),
);
