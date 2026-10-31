import { component, control, file, kernel, poly, port, signature, type Audio, type Events, type Params, type Value } from "@m2/core";
import { adsr, osc, svf, vca } from "std/dsp";
import { sampler } from "std/sampler";

/** A soft pluck, built from std's blocks. */
export const tinyKeys = component(
  {
    id: "tiny-keys",
    label: "Tiny keys",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck, built from std's blocks" },
  },
  // A pack never imports another (PACK-3): a soft pluck, built from std's blocks.
  (notes: Events, { brightness }: Params): Audio =>
    poly(notes, (n) => vca(svf.lowpass(osc.saw(n.pitch), brightness), adsr(n.gate, { decay: 0.45, sustain: 0, release: 0.25 })), { voices: 12 }),
);

/** One recorded pluck, pitched to every note: the recording is the pack's own file. */
export const tinySampler = component(
  {
    id: "tiny-sampler",
    label: "Tiny sampler",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    output: port.audio(),
    editor: { category: "Keys", description: "A recorded pluck, from the pack's own samples" },
  },
  (notes: Events): Audio => sampler(notes, { sample: file(import.meta, "samples/pluck.wav"), mode: "gate", release: 0.1 }),
);

/** A crunch: std's drive kernel, harder. */
export const tinyCrunch = component(
  {
    id: "tiny-crunch",
    label: "Tiny crunch",
    signature: signature<(x: Audio, amount?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { amount: control.db(12).range(0, 36).smooth() },
    output: port.audio(),
    positional: ["amount"],
    editor: { category: "Distortion", description: "std's drive, harder", insert: { amount: 12 } },
  },
  // std's drive kernel: a pack never imports another (PACK-3).
  (x: Audio, { amount }: Params): Audio => kernel("std:drive", { in: x, amount }),
);
