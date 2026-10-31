import { component, control, kernel, poly, port, signature, type Audio, type Events, type Params } from "@m2/core";

/** A soft pluck, on std's synth kernel. */
export const softKeys = component(
  {
    id: "soft-keys",
    label: "Soft keys",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck, on std's synth kernel" },
  },
  // A pack never imports another (PACK-3): std's synth kernel, played as a soft pluck.
  (notes: Events, { brightness }: Params): Audio =>
    poly(notes, (n) => kernel("std:synth", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, cutoff: brightness, decay: 0.45, sustain: 0, release: 0.25 }), { voices: 12 }),
);
