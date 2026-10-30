import { component, control, port, signature, type Audio, type Events, type Params } from "@m2/core";
import { synth } from "std/synth";

/** A warm pad: std's synth pluck, slower. */
export const warmPad = component(
  {
    id: "warm-pad",
    label: "Warm pad",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Pads", description: "A warm pad, from std's synth" },
  },
  (notes: Events, { brightness }: Params): Audio => synth.pluck(notes, { brightness }),
);
