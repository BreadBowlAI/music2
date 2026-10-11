// std/control: modulation and masks.

import { kernel, type Control, type Signal, type Value } from "@m2/core";
import { blend, scale } from "./math";

export { blend, range, xrange } from "./math";

const SHAPES = { sine: 0, tri: 1, saw: 2, square: 3 } as const;

/**
 * A beat-synced LFO, 0..1. `beats` is song time (`time.beats` from the track
 * context); `period` is how many beats one cycle lasts.
 */
export function lfo(beats: Control<"beats">, o: { period?: Value; shape?: keyof typeof SHAPES; phase?: Value } = {}): Control<"unit01"> {
  return kernel("std:lfo", { beats, period: o.period, shape: SHAPES[o.shape ?? "sine"], phase: o.phase }) as any;
}

/** `x` where `mask` is 1, silence (or zero) where it is 0. */
export const when = <S extends Signal>(mask: Value, x: S): S => scale(x, mask);

/** `a` normally, `b` where `mask` is 1. Same as `blend(a, b, mask)`. */
export const choose = <S extends Signal>(mask: Value, a: S, b: S): S => blend(a, b, mask);
