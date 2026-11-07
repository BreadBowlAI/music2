// std/filter: filters to put after any sound, built on the state-variable filter.
//
// Each takes the audio first, then the cutoff in Hz and an optional resonance `q`
// (0.707 is flat; higher rings at the cutoff): `lowpass(x, 1000)`, `lowpass(x, 1000, 2)`
// or `lowpass(x, { q: 2 })`. Both are knobs, written or left out.
//
// Each card's face is its curve, measured from the filter's own sound (CARD-91): one dot moves the
// cutoff across and the q up and down (and by the wheel), over the sound's spectrum as it plays.

import { component, control, kernel, knob, port, signature, type Audio, type Params, type Value } from "@m2/core";
import { add } from "./math";
import { curvePicture, dbWords, hzWords } from "./pictures";

/** The filter's peak, in dB, at its cutoff: the q's own (the state-variable filter rings by q there). */
const peak = (q: number | null) => 20 * Math.log10(Math.max(0.01, q ?? 0.707));
/** What a filter's curve says: what it does, at its cutoff and q. */
const said = (what: string, cutoff: number | null, q: number | null) =>
  `${what} ${cutoff === null ? "a cutoff set elsewhere" : hzWords(cutoff)}, q ${q === null ? "set elsewhere" : (Math.round(q * 100) / 100).toString()} (${dbWords(Math.round(peak(q) * 10) / 10)} at the cutoff)`;

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
    editor: { category: "Filter", description: "Darker: removes the highs above the cutoff", insert: { cutoff: 1000 }, views: { curve: curvePicture() }, face: ["curve", "cutoff", "q"] },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("lp"),
  {
    curve: {
      draw: (v, p) => [p.response({ name: "curve" }), p.handle(v.cutoff, peak(v.q), { x: "cutoff", y: knob("q"), wheel: "q" })],
      words: (v) => said("Cuts the highs above", v.cutoff, v.q),
    },
  },
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
    editor: { category: "Filter", description: "Thinner: removes the lows below the cutoff", insert: { cutoff: 200 }, views: { curve: curvePicture() }, face: ["curve", "cutoff", "q"] },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("hp"),
  {
    curve: {
      draw: (v, p) => [p.response({ name: "curve" }), p.handle(v.cutoff, peak(v.q), { x: "cutoff", y: knob("q"), wheel: "q" })],
      words: (v) => said("Cuts the lows below", v.cutoff, v.q),
    },
  },
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
    editor: { category: "Filter", description: "Keeps only a band around the cutoff, like a telephone", insert: { cutoff: 1000 }, views: { curve: curvePicture() }, face: ["curve", "cutoff", "q"] },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("bp"),
  {
    curve: {
      draw: (v, p) => [p.response({ name: "curve" }), p.handle(v.cutoff, peak(v.q), { x: "cutoff", y: knob("q"), wheel: "q" })],
      words: (v) => said("Keeps a band around", v.cutoff, v.q),
    },
  },
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
    editor: { category: "Filter", description: "Cuts a narrow band out around the cutoff", insert: { cutoff: 1000 }, views: { curve: curvePicture() }, face: ["curve", "cutoff", "q"] },
  },
  (x: Audio, { cutoff, q }: Params) => {
    const f = kernel("std:svf", { in: x, cutoff, q });
    return add(f.port("lp"), f.port("hp"));
  },
  {
    curve: {
      draw: (v, p) => [p.response({ name: "curve" }), p.handle(v.cutoff, -24, { x: "cutoff", y: knob("q"), wheel: "q" })],
      words: (v) => said("Cuts a narrow band out at", v.cutoff, v.q),
    },
  },
);
