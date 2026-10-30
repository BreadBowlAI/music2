import { component, control, port, signature, type Audio, type Events, type Params } from "@m2/core";
import { synth } from "std/synth";

/** A soft pluck: std's synth pluck, darker. */
export const softKeys = component(
  {
    id: "soft-keys",
    label: "Soft keys",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck, from std's synth" },
  },
  (notes: Events, { brightness }: Params): Audio => synth.pluck(notes, { brightness }),
);
