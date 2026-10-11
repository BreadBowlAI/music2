// std/dsp: oscillators, envelopes, filters and amplifiers.

import { component, kernel, Quantity, Signal, type Audio, type Control, type Value } from "@m2/core";
import { mtof } from "./math";

/** Oscillators take a frequency, or a pitch in semitones (converted for you). */
function freq(x: Value): Value {
  if (x instanceof Signal && x.unit === "semitones") return mtof(x);
  if (x instanceof Quantity && x.unit === "semitones") return mtof(x);
  return x;
}

export const osc = {
  sine: component({ controls: ["0"] }, (f: Value): Audio => kernel("std:sine", { freq: freq(f) })),
  saw: component({ controls: ["0"] }, (f: Value): Audio => kernel("std:saw", { freq: freq(f) })),
  square: component({ controls: ["0", "1"] }, (f: Value, width?: Value): Audio => kernel("std:square", { freq: freq(f), width })),
  tri: component({ controls: ["0"] }, (f: Value): Audio => kernel("std:tri", { freq: freq(f) })),
};

/** White noise. The same seed always gives the same noise. */
export const noise = (seed?: Value): Audio => kernel("std:noise", { seed });

export type AdsrOptions = { attack?: Value; decay?: Value; sustain?: Value; release?: Value };

/** Attack/decay/sustain/release envelope, 0..1, driven by a gate such as `note.gate`. */
export const adsr = component(
  { controls: ["1.attack", "1.decay", "1.sustain", "1.release"] },
  (gate: Value, o: AdsrOptions = {}): Control<"unit01"> => kernel("std:adsr", { gate, ...o }) as any,
);

/** Envelope follower: how loud `x` is right now, 0..1. */
export const follow = component(
  { controls: ["1.attack", "1.release"] },
  (x: Audio, o: { attack?: Value; release?: Value } = {}): Control<"unit01"> => kernel("std:follow", { in: x, ...o }) as any,
);

/** State-variable filter. */
export const svf = {
  lowpass: component({ controls: ["1", "2"] }, (x: Audio, cutoff: Value, q?: Value): Audio => kernel("std:svf", { in: x, cutoff, q }).port("lp")),
  bandpass: component({ controls: ["1", "2"] }, (x: Audio, cutoff: Value, q?: Value): Audio => kernel("std:svf", { in: x, cutoff, q }).port("bp")),
  highpass: component({ controls: ["1", "2"] }, (x: Audio, cutoff: Value, q?: Value): Audio => kernel("std:svf", { in: x, cutoff, q }).port("hp")),
};

/** Multiply audio by a gain (an envelope, a mask, a number). */
export const vca = component({ controls: ["1"] }, (x: Audio, gain: Value): Audio => kernel("std:scale", { in: x, by: gain }));

/** Gain in dB. */
export const gain = component({ controls: ["1"] }, (x: Audio, db: Value): Audio => kernel("std:gain", { in: x, db }));

/** Mono to stereo, -1 (left) to 1 (right). */
export const pan = component({ controls: ["1"] }, (x: Audio, position: Value): Audio => kernel("std:pan", { in: x, pan: position }));
