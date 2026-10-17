// std/mix: summing tracks into the master.

import { component, control, each, kernel, port, signature, type Audio, type Params, type Signal, type Value } from "@m2/core";
import { gain, pan } from "./dsp";
import { limit } from "./fx";
import { sum } from "./math";

/** A mixer channel: its audio, or its audio with a gain (dB) and a pan. */
export type Channel = Audio | { in: Audio; db?: Value; pan?: Value };

/**
 * Sum channels, each with a gain (dB, 0 when left out) and an optional pan (a stereo channel is
 * balanced). −96 dB or lower is silence. No channels is silence.
 * Each channel's key names it, so `hats.db` is the hats fader (PKG-6).
 */
export const mixer = component(
  {
    id: "mixer",
    label: "Mixer",
    signature: signature<(channels: Record<string, Channel>) => Audio>(),
    controls: {
      channels: each({
        in: port.audio(),
        // Every channel has a gain, its fader: at 0 dB it is `x × 1.0`, which changes no bit.
        db: control.db(0).range(-60, 12),
        // Optional: a channel gets a panner only when it says so, as a mono sound through a
        // centred panner isn't bit for bit the same (PKG-5).
        pan: control.bipolar(0).range(-1, 1).optional().smooth(),
      }),
    },
    output: port.audio(),
    positional: ["channels"],
  },
  function mixer({ channels }: Params): Audio {
    // In the caller's key order, as written.
    const strips = Object.values(channels as Record<string, { in: Audio; db: Signal; pan?: Signal }>).map((c) =>
      gain(c.pan === undefined ? c.in : pan(c.in, c.pan), c.db),
    );
    // A song with no tracks yet (the Empty template) still plays: a sine at 0 Hz, times 0.
    if (strips.length === 0) return kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 });
    return sum(...strips);
  },
);

/** The end of the chain: a gain (dB), then a limiter so the song never clips. −96 dB or lower is silence. */
export const master = component(
  {
    id: "master",
    label: "Master",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { db: control.db(0).range(-60, 12), ceiling: control.db(-1).range(-24, 0) },
    output: port.audio(),
  },
  // Stereo from the start, sample for sample: a mono mix whose first channel is panned turns
  // stereo, and the gain and limiter keep going rather than start again (design-8b §5.1).
  (x: Audio, { db, ceiling }: Params) => limit(gain(kernel("std:stereo", { in: x }), db), ceiling),
);
