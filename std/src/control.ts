// std/control: modulation and masks.

import { component, control, kernel, port, setting, signature, type Control, type Params, type Signal, type Value } from "@m2/core";
import { blend, scale } from "./math";

export { blend, range, xrange } from "./math";

const SHAPES = { sine: 0, tri: 1, saw: 2, square: 3 } as const;

/**
 * A beat-synced LFO, 0..1. `beats` is song time (`time.beats` from the track
 * context); `period` is how many beats one cycle lasts.
 */
export const lfo = component(
  {
    id: "lfo",
    label: "LFO",
    signature: signature<(beats: Control<"beats">) => Control<"unit01">>(),
    inputs: { beats: port.control("beats") },
    // `phase` is read every sample, so a step jumps the output (PKG-14). `period` isn't smooth:
    // the kernel reads the phase off the song's beats, so a ramp of it would sweep the LFO
    // through many cycles in one block; gliding it would be a kernel change.
    controls: {
      period: control.beats(1).range(1 / 16, 16).log(),
      phase: control.frac(0).range(0, 1).smooth(),
    },
    // Fixed when the song builds (changing it rebuilds): a setting, not a knob (D3). The kernel takes it as a number.
    settings: { shape: setting.choice(["sine", "tri", "saw", "square"], "sine") },
    output: port.control("unit01"),
  },
  (beats: Control<"beats">, { period, shape, phase }: Params) =>
    kernel("std:lfo", { beats, period, shape: SHAPES[shape as keyof typeof SHAPES], phase }),
);

/** `x` where `mask` is 1, silence (or zero) where it is 0. */
export const when = component(
  {
    id: "when",
    label: "When",
    signature: signature<<S extends Signal>(mask: Value, x: S) => S>(),
    inputs: { x: port.any() },
    controls: { mask: control.frac().range(0, 1).smooth() },
    output: port.any(),
    positional: ["mask", "x"],
  },
  (x: Signal, { mask }: Params) => scale(x, mask),
);

/** `a` normally, `b` where `mask` is 1. Same as `blend(a, b, mask)`. */
export const choose = component(
  {
    id: "choose",
    label: "Choose",
    signature: signature<<S extends Signal>(mask: Value, a: S, b: S) => S>(),
    inputs: { a: port.any(), b: port.any() },
    controls: { mask: control.frac().range(0, 1).smooth() },
    output: port.any(),
    positional: ["mask", "a", "b"],
  },
  (a: Signal, b: Signal, { mask }: Params) => blend(a, b, mask),
);
