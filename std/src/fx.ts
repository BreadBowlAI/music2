// std/fx: effects.

import { kernel, type Audio, type Value } from "@m2/core";

/** Turn `x` down by `depth` dB while `side` is high. Use another track's `follow()` export as `side`. */
export const duck = (x: Audio, side: Value, o: { depth?: Value } = {}): Audio => kernel("std:duck", { in: x, side, depth: o.depth });

/** Feedback delay. `time` up to 2 seconds; `mix` 0 is dry, 1 is only echoes. */
export const delay = (x: Audio, o: { time?: Value; feedback?: Value; mix?: Value } = {}): Audio =>
  kernel("std:delay", { in: x, time: o.time, feedback: o.feedback, mix: o.mix });

/** Soft saturation; `amount` is dB of drive. */
export const drive = (x: Audio, amount?: Value): Audio => kernel("std:drive", { in: x, amount });

/** Peak limiter with a hard ceiling in dB. */
export const limit = (x: Audio, ceiling?: Value): Audio => kernel("std:limit", { in: x, ceiling });
