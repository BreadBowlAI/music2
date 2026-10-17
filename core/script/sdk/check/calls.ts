// Every call form a component's derived type must take, and the mistakes it must refuse
// (PKG-1, the step 5 design's D15). A line that should fail carries `@ts-expect-error`, so a
// type that stops refusing it fails the check too.

import { component, control, each, kernel, port, setting, signature, hz, ms, time, type Audio, type Control, type Ctx, type Params, type Signal, type Value } from "@m2/core";
import { osc } from "std/dsp";
import { mixer } from "std/mix";

/** Positional slots after the audio, with a signature for their names. */
export const lowpass = component(
  {
    id: "lowpass",
    signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { cutoff: control.hz(1000).range(20, 20000).log().smooth(), q: control.scalar(0.707).range(0.3, 20).log().smooth() },
    output: port.audio(),
    positional: ["cutoff", "q"],
    editor: { category: "Filter", insert: { cutoff: 1000 } },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("lp"),
);

/** Only an options object after the audio; no signature. */
export const delay = component(
  {
    id: "delay",
    inputs: { in: port.audio() },
    controls: { time: control.ms(250).range(1, 2000).log().smooth(), feedback: control.frac(0.3), mix: control.frac(0.3) },
    output: port.audio(),
  },
  (x: Audio, { time, feedback, mix }: Params) => kernel("std:delay", { in: x, time, feedback, mix }),
);

export const square = component(
  {
    id: "square",
    signature: signature<(freq?: Value, width?: Value) => Audio>(),
    controls: { freq: control.hz(440).accepts("semitones", 0, 127), width: control.frac(0.5) },
    output: port.audio(),
    positional: ["freq", "width"],
  },
  ({ freq, width }: Params) => kernel("std:square", { freq, width }),
);

/** Generic helpers keep their type parameters: every slot is positional, at most one optional. */
export const when = component(
  { id: "when", signature: signature<<S extends Signal>(mask: Value, x: S) => S>(), inputs: { x: port.any() }, controls: { mask: control.frac() }, positional: ["mask", "x"] },
  (x: Signal, { mask }: Params) => kernel("std:scale", { in: x, by: mask }),
);
export const scale = component(
  { id: "scale", signature: signature<<S extends Signal>(x: S | Value, by?: Value) => S>(), inputs: { x: port.any() }, controls: { by: control.linear(1) }, positional: ["x", "by"] },
  (x: Signal, { by }: Params) => kernel("std:scale", { in: x, by }) as any,
);
export const sum = component(
  { id: "sum", signature: signature<<S extends Signal>(...xs: S[]) => S>(), inputs: { xs: port.any().many() } },
  (xs: Signal[]) => xs[0],
);

export const noise = component({ id: "noise", settings: { seed: setting.int(1) }, positional: ["seed"], output: port.audio() }, ({ seed }: Params) => kernel("std:noise", { seed }));

export const lfo = component(
  {
    id: "lfo",
    inputs: { beats: port.control("beats") },
    controls: { period: control.beats(1) },
    settings: { shape: setting.choice(["sine", "tri"], "sine") },
    output: port.control("unit01"),
  },
  (b: Signal, { period, shape }: Params) => kernel("std:lfo", { beats: b, period, shape: shape === "sine" ? 0 : 1 }),
);

export const mix = component(
  { id: "mix", controls: { channels: each({ in: port.audio(), db: control.db(0).optional(), pan: control.bipolar(0).optional() }) }, positional: ["channels"], output: port.audio() },
  ({ channels }: Params) => channels,
);

/** Two leading inputs: their order isn't in the types, so any arguments are taken. */
export const duck = component(
  { id: "duck", inputs: { in: port.audio(), side: port.control("unit01", { sidechain: true }) }, controls: { depth: control.db(-8) }, output: port.audio() },
  (x: Audio) => x,
);

export const mtof = component({ id: "mtof", controls: { pitch: control.semis() }, positional: ["pitch"], output: port.control("hz") }, ({ pitch }: Params) => kernel("std:mtof", { pitch }));

declare const x: Audio, wobble: Signal, beats: Control<"beats">, side: Control<"unit01">;
// Options a helper forwards and may not have been given: the SDK builds these too.
declare const echo: { time?: number; mix?: number } | undefined, tone: { q?: number } | undefined;

// What songs and the planner write.
export const ok = [
  lowpass(x),
  lowpass(x, 1000),
  lowpass(x, 1000, 0.8),
  lowpass(x, { q: 0.8 }),
  lowpass(x, 1000, { q: 0.8 }),
  lowpass(x, { q: wobble }),
  lowpass(x, undefined, 0.8),
  lowpass(x, hz(800)),
  delay(x),
  delay(x, { time: 0.25, feedback: 0.3, mix: 0.3 }),
  delay(x, { time: ms(250) }),
  delay(x, echo),
  lowpass(x, 1000, 0.8, tone),
  square(),
  square(440),
  square({ width: 0.5 }),
  square(440, { width: 0.5 }),
  square(undefined, 0.5),
  when(0.5, x),
  noise(7),
  noise(),
  sum(x, x, x),
  mtof(60),
  lfo(beats, { shape: "tri", period: 2 }),
  lfo(beats),
  mix({ kick: x, hats: { in: x, db: -6, pan: 0.3 } }),
  duck(x, side, { depth: -9 }),
  scale(x),
  scale(x, 0.5),
  osc.saw(110),
  mixer({ kick: x }),
];

// The generic helpers' results keep their kind.
export const a: Audio = when(0.5, x);
export const b: Audio = scale(x, 2);
export const c: Audio = sum(x, x);
export const d: Control<"unit01"> = lfo(beats);

// Song time in a component's body, and `ctx.time`, which is the same.
export const playing: Control<"unit01"> = time.playing;
export const position: [Control<"beats">, Control<"seconds">] = [time.beats, time.seconds];
export const same = (ctx: Ctx): Control<"unit01"> => ctx.time.playing;
// @ts-expect-error playing is 0 or 1, not a position
export const notBeats: Control<"beats"> = time.playing;
// @ts-expect-error time is read, never written
time.playing = side;

// @ts-expect-error an unknown option
delay(x, { tme: 1 });
// @ts-expect-error a required input left out
when(0.5);
// @ts-expect-error an options object, then a positional argument
lowpass(x, { q: 1 }, 0.8);
// @ts-expect-error a choice that isn't offered
lfo(beats, { shape: "saw" });
// @ts-expect-error too many arguments
lowpass(x, 1, 2, 3);
// @ts-expect-error a string where a value goes
lowpass(x, "1000");
// @ts-expect-error a required control left out
mtof();
// @ts-expect-error the input left out
delay();
// @ts-expect-error a dictionary entry with a field it doesn't have
mix({ hats: { in: x, gain: 1 } });

// @ts-expect-error a signature listing 2 parameters for a descriptor that has 3
export const drifted = component({ id: "drifted", signature: signature<(x: Audio, cutoff?: Value) => Audio>(), inputs: { in: port.audio() }, controls: { cutoff: control.hz(1000), q: control.scalar(0.7) }, output: port.audio(), positional: ["cutoff", "q"] }, (y: Audio) => y);
