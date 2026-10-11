// std/fx: effects.

import { kernel, type Audio, type Value } from "@m2/core";

/** Turn `x` down by `depth` dB while `side` is high. Use another track's `follow()` export as `side`. */
export const duck = (x: Audio, side: Value, o: { depth?: Value } = {}): Audio => kernel("std:duck", { in: x, side, depth: o.depth });

/** Soft saturation; `amount` is dB of drive. */
export const drive = (x: Audio, amount?: Value): Audio => kernel("std:drive", { in: x, amount });

/** Peak limiter with a hard ceiling in dB. */
export const limit = (x: Audio, ceiling?: Value): Audio => kernel("std:limit", { in: x, ceiling });
