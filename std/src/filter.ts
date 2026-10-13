// std/filter: filters to put after any sound, built on the state-variable filter.
//
// Each takes the audio first, then the cutoff in Hz and an optional resonance `q`
// (0.707 is flat; higher rings at the cutoff): `lowpass(x, 1000)`, `lowpass(x, 1000, 2)`
// or `lowpass(x, { q: 2 })`. Both are knobs, written or left out.

import { component, control, kernel, port, signature, type Audio, type Params, type Value } from "@m2/core";
import { add } from "./math";

// The kernel reads both every sample without smoothing, so a step would click (PKG-14).
const CUTOFF = control.hz(1000).range(20, 20000).log().smooth();
const Q = control.scalar(0.707).range(0.3, 20).log().smooth();

/** Darker: removes the highs above `cutoff`. */
export const lowpass = component(
  {
    id: "lowpass",
    label: "Lowpass",
    signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { cutoff: CUTOFF, q: Q },
    output: port.audio(),
    positional: ["cutoff", "q"],
    editor: { category: "Filter", description: "Darker: removes the highs above the cutoff", insert: { cutoff: 1000 } },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("lp"),
);

/** Thinner: removes the lows below `cutoff`. */
export const highpass = component(
  {
    id: "highpass",
    label: "Highpass",
    signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { cutoff: CUTOFF, q: Q },
    output: port.audio(),
    positional: ["cutoff", "q"],
    editor: { category: "Filter", description: "Thinner: removes the lows below the cutoff", insert: { cutoff: 200 } },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("hp"),
);

/** Keeps only a band around `cutoff`; a higher `q` makes it narrower. */
export const bandpass = component(
  {
    id: "bandpass",
    label: "Bandpass",
    signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { cutoff: CUTOFF, q: Q },
    output: port.audio(),
    positional: ["cutoff", "q"],
    editor: { category: "Filter", description: "Keeps only a band around the cutoff, like a telephone", insert: { cutoff: 1000 } },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("bp"),
);

/** Cuts a narrow band out around `cutoff`: the lowpass and highpass of one filter, summed. */
export const notch = component(
  {
    id: "notch",
    label: "Notch",
    signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { cutoff: CUTOFF, q: Q },
    output: port.audio(),
    positional: ["cutoff", "q"],
    editor: { category: "Filter", description: "Cuts a narrow band out around the cutoff", insert: { cutoff: 1000 } },
  },
  (x: Audio, { cutoff, q }: Params) => {
    const f = kernel("std:svf", { in: x, cutoff, q });
    return add(f.port("lp"), f.port("hp"));
  },
);
