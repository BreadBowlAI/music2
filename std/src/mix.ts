// std/mix: summing tracks into the master.

import { db, type Audio, type Value } from "@m2/core";
import { gain, pan } from "./dsp";
import { limit } from "./fx";
import { sum } from "./math";

export type Channel = Audio | { in: Audio; db?: Value; pan?: Value };

/** Sum channels, each with optional gain (dB) and pan (mono channels only). */
export function mixer(channels: Record<string, Channel>): Audio {
  const strips = Object.entries(channels).map(([, c]) => {
    if (!("in" in c)) return c;
    let x = c.in;
    if (c.pan !== undefined) x = pan(x, c.pan);
    if (c.db !== undefined) x = gain(x, c.db);
    return x;
  });
  return sum(...strips);
}

/** The end of the chain: a limiter so the song never clips. */
export const master = (x: Audio, o: { ceiling?: Value } = {}): Audio => limit(x, o.ceiling ?? db(-1));
