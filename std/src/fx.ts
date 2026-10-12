// std/fx: effects.

import { component, kernel, type Audio, type Value } from "@m2/core";

/** Turn `x` down by `depth` dB while `side` is high. Use another track's `follow()` export as `side`. */
export const duck = component({ controls: ["2.depth"] }, (x: Audio, side: Value, o: { depth?: Value } = {}): Audio => kernel("std:duck", { in: x, side, depth: o.depth }));

/** Feedback delay. `time` up to 2 seconds; `mix` 0 is dry, 1 is only echoes. */
export const delay = component(
  { controls: ["1.time", "1.feedback", "1.mix"] },
  (x: Audio, o: { time?: Value; feedback?: Value; mix?: Value } = {}): Audio => kernel("std:delay", { in: x, time: o.time, feedback: o.feedback, mix: o.mix }),
);

/** Soft saturation; `amount` is dB of drive. */
export const drive = component({ controls: ["1"] }, (x: Audio, amount?: Value): Audio => kernel("std:drive", { in: x, amount }));

/** Peak limiter with a hard ceiling in dB. */
export const limit = component({ controls: ["1"] }, (x: Audio, ceiling?: Value): Audio => kernel("std:limit", { in: x, ceiling }));

// ---------------------------------------------------------------- space

/**
 * Reverb: the sound of a room around `x`. `size` 0..1 goes from a small room to a huge
 * hall, `damping` 0..1 from a bright tail to a dark one, and `mix` 0..1 from dry to only
 * reverb. A stereo sound gets a stereo tail; a mono one stays mono, so it can still be panned.
 */
export const reverb = component(
  { controls: ["1.size", "1.damping", "1.mix"] },
  (x: Audio, o: { size?: Value; damping?: Value; mix?: Value } = {}): Audio => kernel("std:reverb", { in: x, size: o.size, damping: o.damping, mix: o.mix }),
);

// ---------------------------------------------------------------- modulation

/** Chorus: thickens `x` with slowly drifting copies (spread left and right if `x` is stereo). `rate` in Hz; `depth` and `mix` 0..1. */
export const chorus = component(
  { controls: ["1.rate", "1.depth", "1.mix"] },
  (x: Audio, o: { rate?: Value; depth?: Value; mix?: Value } = {}): Audio => kernel("std:chorus", { in: x, rate: o.rate, depth: o.depth, mix: o.mix }),
);

/** Phaser: sweeping notches. `rate` in Hz; `depth`, `feedback` (a sharper sound) and `mix` 0..1, where 0.5 sweeps deepest. */
export const phaser = component(
  { controls: ["1.rate", "1.depth", "1.feedback", "1.mix"] },
  (x: Audio, o: { rate?: Value; depth?: Value; feedback?: Value; mix?: Value } = {}): Audio =>
    kernel("std:phaser", { in: x, rate: o.rate, depth: o.depth, feedback: o.feedback, mix: o.mix }),
);

/** Tremolo: the volume pulses `rate` times a second, dipping by `depth` (1 is down to silence). */
export const tremolo = component(
  { controls: ["1.rate", "1.depth"] },
  (x: Audio, o: { rate?: Value; depth?: Value } = {}): Audio => kernel("std:tremolo", { in: x, rate: o.rate, depth: o.depth }),
);

// ---------------------------------------------------------------- dynamics and tone

/**
 * Compressor: above `threshold` (dB) the level rises only 1/`ratio` as fast, so loud and
 * quiet moments even out. `attack` and `release` (seconds) are how fast it grabs and lets
 * go; `makeup` (dB) brings the level back up.
 */
export const compress = component(
  { controls: ["1.threshold", "1.ratio", "1.attack", "1.release", "1.makeup"] },
  (x: Audio, o: { threshold?: Value; ratio?: Value; attack?: Value; release?: Value; makeup?: Value } = {}): Audio =>
    kernel("std:compress", { in: x, threshold: o.threshold, ratio: o.ratio, attack: o.attack, release: o.release, makeup: o.makeup }),
);

/** Three-band EQ: `low`, `mid` and `high` in dB (0 leaves the sound alone), around `lowFreq`, `midFreq` and `highFreq` in Hz. */
export const eq3 = component(
  { controls: ["1.low", "1.mid", "1.high", "1.lowFreq", "1.midFreq", "1.highFreq"] },
  (x: Audio, o: { low?: Value; mid?: Value; high?: Value; lowFreq?: Value; midFreq?: Value; highFreq?: Value } = {}): Audio =>
    kernel("std:eq3", { in: x, low: o.low, mid: o.mid, high: o.high, low_freq: o.lowFreq, mid_freq: o.midFreq, high_freq: o.highFreq }),
);

/** Bitcrusher: `bits` of resolution (16 is clean, 4 is harsh) at a lower sample `rate` in Hz, like an old sampler. `mix` 0..1. */
export const bitcrush = component(
  { controls: ["1.bits", "1.rate", "1.mix"] },
  (x: Audio, o: { bits?: Value; rate?: Value; mix?: Value } = {}): Audio => kernel("std:bitcrush", { in: x, bits: o.bits, rate: o.rate, mix: o.mix }),
);

// ---------------------------------------------------------------- stereo

/** Stereo width: 0 is mono, 1 leaves it alone, 2 is twice as wide. The result is stereo: a mono sound gets width too, and still sums cleanly back to mono. */
export const widen = component({ controls: ["1.width"] }, (x: Audio, o: { width?: Value } = {}): Audio => kernel("std:widen", { in: x, width: o.width }));
