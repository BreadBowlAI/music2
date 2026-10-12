// std/mix: summing tracks into the master.

import { component, db, kernel, type Audio, type Value } from "@m2/core";
import { gain, pan } from "./dsp";
import { limit } from "./fx";
import { sum } from "./math";

export type Channel = Audio | { in: Audio; db?: Value; pan?: Value };

/** Sum channels, each with optional gain (dB) and pan (a stereo channel is balanced). No channels is silence. */
export const mixer = component({ controls: ["0.*.db", "0.*.pan"] }, function mixer(channels: Record<string, Channel>): Audio {
  const strips = Object.entries(channels).map(([, c]) => {
    if (!("in" in c)) return c;
    let x = c.in;
    if (c.pan !== undefined) x = pan(x, c.pan);
    if (c.db !== undefined) x = gain(x, c.db);
    return x;
  });
  // A song with no tracks yet (the Empty template) still plays: a sine at 0 Hz, times 0.
  if (strips.length === 0) return kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 });
  return sum(...strips);
});

/** The end of the chain: a limiter so the song never clips. */
export const master = component({ controls: ["1.ceiling"] }, (x: Audio, o: { ceiling?: Value } = {}): Audio => limit(x, o.ceiling ?? db(-1)));
