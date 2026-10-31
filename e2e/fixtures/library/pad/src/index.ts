import { component, control, poly, port, signature, type Audio, type Events, type Params } from "@m2/core";
import { adsr, osc, svf, vca } from "std/dsp";

/** A warm pad, built from std's blocks. */
export const warmPad = component(
  {
    id: "warm-pad",
    label: "Warm pad",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Pads", description: "A warm pad, built from std's blocks" },
  },
  // A pack never imports another (PACK-3): a soft pluck, built from std's blocks.
  (notes: Events, { brightness }: Params): Audio =>
    poly(notes, (n) => vca(svf.lowpass(osc.saw(n.pitch), brightness), adsr(n.gate, { decay: 0.45, sustain: 0, release: 0.25 })), { voices: 12 }),
);
