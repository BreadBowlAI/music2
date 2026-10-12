// std/math: arithmetic on signals and unit conversions.
//
// Results keep the unit of their inputs where it is known, so `add(note.pitch, 12)`
// is still a pitch and `osc.saw` converts it to a frequency.

import { component, kernel, unitOf, withUnit, type Control, type Signal, type Value } from "@m2/core";

/** `a + b`: two audio signals, or two controls of the same unit. */
export const add = component({ controls: ["0", "1"] }, <S extends Signal>(a: S | Value, b: S | Value): S => withUnit(kernel("std:add", { a, b }), unitOf(a) ?? unitOf(b)) as any);

/** `x × by` (a gain). Works on audio and on controls. */
export const scale = component({ controls: ["1"] }, <S extends Signal>(x: S | Value, by: Value): S => withUnit(kernel("std:scale", { in: x, by }), unitOf(x)) as any);

/** Crossfade: `a` where `m` is 0, `b` where it is 1. */
export const blend = component({ controls: ["0", "1", "2"] }, <S extends Signal>(a: S | Value, b: S | Value, m: Value): S =>
  withUnit(kernel("std:blend", { a, b, m }), unitOf(a) ?? unitOf(b)) as any);

// Helpers that build nodes are components too, even without controls: the editor shows
// a call of one as a node, with the signals it reads as its inputs.

/** Sum any number of signals of the same kind. */
export const sum = component({ controls: [] }, <S extends Signal>(...xs: S[]): S => {
  if (xs.length === 0) throw new Error("sum() needs at least one signal");
  return xs.slice(1).reduce((acc, x) => add(acc, x), xs[0]);
});

/** Pitch in semitones (60 = middle C) to frequency. A node, so a written pitch stays editable. */
export const mtof = component({ controls: ["0"] }, (pitch: Value): Control<"hz"> => withUnit(kernel("std:mtof", { pitch }), "hz") as any);

/** Map a 0..1 control onto lo..hi. */
export const range = component({ controls: ["1", "2"] }, (x: Value, lo: Value, hi: Value): Control => withUnit(kernel("std:range", { x, lo, hi }), unitOf(lo) ?? unitOf(hi)) as any);

/** Map a 0..1 control onto lo..hi exponentially, which sounds even for frequencies. */
export const xrange = component({ controls: ["1", "2"] }, (x: Value, lo: Value, hi: Value): Control => withUnit(kernel("std:xrange", { x, lo, hi }), unitOf(lo) ?? unitOf(hi)) as any);
