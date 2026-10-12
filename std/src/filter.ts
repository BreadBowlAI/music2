// std/filter: filters to put after any sound, built on the state-variable filter.
//
// Each takes the audio first, then the cutoff in Hz and an optional resonance `q`
// (0.707 is flat; higher rings at the cutoff). Both are knobs when written as numbers.

import { component, kernel, type Audio, type Value } from "@m2/core";
import { add } from "./math";

/** Darker: removes the highs above `cutoff`. */
export const lowpass = component({ controls: ["1", "2"] }, (x: Audio, cutoff?: Value, q?: Value): Audio => kernel("std:svf", { in: x, cutoff, q }).port("lp"));

/** Thinner: removes the lows below `cutoff`. */
export const highpass = component({ controls: ["1", "2"] }, (x: Audio, cutoff?: Value, q?: Value): Audio => kernel("std:svf", { in: x, cutoff, q }).port("hp"));

/** Keeps only a band around `cutoff`; a higher `q` makes it narrower. */
export const bandpass = component({ controls: ["1", "2"] }, (x: Audio, cutoff?: Value, q?: Value): Audio => kernel("std:svf", { in: x, cutoff, q }).port("bp"));

/** Cuts a narrow band out around `cutoff`: the lowpass and highpass of one filter, summed. */
export const notch = component({ controls: ["1", "2"] }, (x: Audio, cutoff?: Value, q?: Value): Audio => {
  const f = kernel("std:svf", { in: x, cutoff, q });
  return add(f.port("lp"), f.port("hp")) as Audio;
});
