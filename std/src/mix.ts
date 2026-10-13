// std/mix: summing tracks into the master.

import { component, control, each, kernel, port, signature, type Audio, type Params, type Signal, type Value } from "@m2/core";
import { gain, pan } from "./dsp";
import { limit } from "./fx";
import { sum } from "./math";

/** A mixer channel: its audio, or its audio with a gain (dB) and a pan. */
export type Channel = Audio | { in: Audio; db?: Value; pan?: Value };

/**
 * Sum channels, each with optional gain (dB) and pan (a stereo channel is balanced). No channels is silence.
 * Each channel's key names it, so `hats.db` is the hats fader (PKG-6).
 */
export const mixer = component(
  {
    id: "mixer",
    label: "Mixer",
    signature: signature<(channels: Record<string, Channel>) => Audio>(),
    controls: {
      // Optional: a channel gets a gain or a panner only when it says so (PKG-5).
      channels: each({
        in: port.audio(),
        db: control.db(0).range(-60, 12).optional(),
        pan: control.bipolar(0).range(-1, 1).optional().smooth(),
      }),
    },
    output: port.audio(),
    positional: ["channels"],
  },
  function mixer({ channels }: Params): Audio {
    // In the caller's key order, as written.
    const strips = Object.values(channels as Record<string, { in: Audio; db?: Signal; pan?: Signal }>).map((c) => {
      let x = c.in;
      if (c.pan !== undefined) x = pan(x, c.pan);
      if (c.db !== undefined) x = gain(x, c.db);
      return x;
    });
    // A song with no tracks yet (the Empty template) still plays: a sine at 0 Hz, times 0.
    if (strips.length === 0) return kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 });
    return sum(...strips);
  },
);

/** The end of the chain: a limiter so the song never clips. */
export const master = component(
  {
    id: "master",
    label: "Master",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { ceiling: control.db(-1).range(-24, 0) },
    output: port.audio(),
  },
  (x: Audio, { ceiling }: Params) => limit(x, ceiling),
);
