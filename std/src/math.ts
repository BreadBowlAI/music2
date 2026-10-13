// std/math: arithmetic on signals and unit conversions.
//
// Results keep the unit of their inputs where it is known, so `add(note.pitch, 12)`
// is still a pitch and `osc.saw` converts it to a frequency.
//
// The generic helpers declare every slot positional, so their calls keep their type
// parameters: `scale(x, 0.5)` is audio when `x` is.

import { component, control, kernel, port, signature, unitOf, withUnit, type Control, type Params, type Signal, type Value } from "@m2/core";

/** `a + b`: two audio signals, or two controls of the same unit. */
export const add = component(
  {
    id: "add",
    label: "Add",
    signature: signature<<S extends Signal>(a: S | Value, b: S | Value) => S>(),
    // Read every sample: a step jumps the sum, a click where it sets a gain (PKG-14).
    controls: { a: control.any().smooth(), b: control.any().smooth() },
    output: port.any(),
    positional: ["a", "b"],
  },
  ({ a, b }: Params) => withUnit(kernel("std:add", { a, b }), unitOf(a) ?? unitOf(b)),
);

/** `x × by` (a gain). Works on audio and on controls. */
export const scale = component(
  {
    id: "scale",
    label: "Scale",
    signature: signature<<S extends Signal>(x: S | Value, by?: Value) => S>(),
    inputs: { x: port.any() },
    // Read every sample: a step would click (PKG-14).
    controls: { by: control.linear(1).range(0, 2).smooth() },
    output: port.any(),
    positional: ["by"],
  },
  (x: Value, { by }: Params) => withUnit(kernel("std:scale", { in: x, by }), unitOf(x)),
);

/** Crossfade: `a` where `m` is 0, `b` where it is 1. */
export const blend = component(
  {
    id: "blend",
    label: "Blend",
    signature: signature<<S extends Signal>(a: S | Value, b: S | Value, m: Value) => S>(),
    controls: { a: control.any().smooth(), b: control.any().smooth(), m: control.frac().range(0, 1).smooth() },
    output: port.any(),
    positional: ["a", "b", "m"],
  },
  ({ a, b, m }: Params) => withUnit(kernel("std:blend", { a, b, m }), unitOf(a) ?? unitOf(b)),
);

// Helpers that build nodes are components too, even without controls: the editor shows
// a call of one as a node, with the signals it reads as its inputs.

/** Sum any number of signals of the same kind. */
export const sum = component(
  {
    id: "sum",
    label: "Sum",
    signature: signature<<S extends Signal>(...xs: S[]) => S>(),
    inputs: { xs: port.any().many() },
    output: port.any(),
  },
  (xs: Signal[]) => {
    if (xs.length === 0) throw new Error("sum() needs at least one signal");
    return xs.slice(1).reduce((acc, x) => add(acc, x), xs[0]);
  },
);

/** Pitch in semitones (60 = middle C) to frequency. A node, so a written pitch stays editable. */
export const mtof = component(
  {
    id: "mtof",
    label: "MTOF",
    signature: signature<(pitch: Value) => Control<"hz">>(),
    controls: { pitch: control.semis().range(0, 127) },
    output: port.control("hz"),
    positional: ["pitch"],
  },
  ({ pitch }: Params) => withUnit(kernel("std:mtof", { pitch }), "hz"),
);

/** Map a 0..1 control onto lo..hi. */
export const range = component(
  {
    id: "range",
    label: "Range",
    signature: signature<(x: Value, lo: Value, hi: Value) => Control>(),
    inputs: { x: port.control("unit01") },
    // Read every sample: a step jumps what the range drives, a gain or a cutoff (PKG-14).
    controls: { lo: control.any().smooth(), hi: control.any().smooth() },
    output: port.control(),
    positional: ["lo", "hi"],
  },
  (x: Value, { lo, hi }: Params) => withUnit(kernel("std:range", { x, lo, hi }), unitOf(lo) ?? unitOf(hi)),
);

/** Map a 0..1 control onto lo..hi exponentially, which sounds even for frequencies. */
export const xrange = component(
  {
    id: "xrange",
    label: "Xrange",
    signature: signature<(x: Value, lo: Value, hi: Value) => Control>(),
    inputs: { x: port.control("unit01") },
    controls: { lo: control.any().smooth(), hi: control.any().smooth() },
    output: port.control(),
    positional: ["lo", "hi"],
  },
  (x: Value, { lo, hi }: Params) => withUnit(kernel("std:xrange", { x, lo, hi }), unitOf(lo) ?? unitOf(hi)),
);
