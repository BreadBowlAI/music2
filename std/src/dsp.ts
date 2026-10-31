// std/dsp: oscillators, envelopes, filters and amplifiers, and `mono` to play one note at a time:
// the blocks a voice of your own is made of.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Control, type Events, type Params, type Signal, type Value } from "@m2/core";
import { mtof } from "./math";

/** Oscillators take a frequency, or a pitch in semitones (converted for you). */
function freq(x: Signal): Value {
  return x.unit === "semitones" ? mtof(x) : x;
}

// A frequency knob shows hertz; a pitch written in semitones (`semis(60)`) shows as a pitch.
const FREQ = control.hz(440).range(20, 20000).log().accepts("semitones", 0, 127);

export const osc = {
  sine: component(
    { id: "sine", label: "Sine", signature: signature<(freq?: Value) => Audio>(), controls: { freq: FREQ }, output: port.audio(), positional: ["freq"] },
    ({ freq: f }: Params) => kernel("std:sine", { freq: freq(f) }),
  ),
  saw: component(
    { id: "saw", label: "Saw", signature: signature<(freq?: Value) => Audio>(), controls: { freq: FREQ }, output: port.audio(), positional: ["freq"] },
    ({ freq: f }: Params) => kernel("std:saw", { freq: freq(f) }),
  ),
  square: component(
    {
      id: "square",
      label: "Square",
      signature: signature<(freq?: Value, width?: Value) => Audio>(),
      // A step of the width moves an edge mid-cycle (PKG-14). One of the frequency only changes
      // a rate, and the phase runs on, so it steps.
      controls: { freq: FREQ, width: control.frac(0.5).range(0.05, 0.95).smooth() },
      output: port.audio(),
      positional: ["freq", "width"],
    },
    ({ freq: f, width }: Params) => kernel("std:square", { freq: freq(f), width }),
  ),
  tri: component(
    { id: "tri", label: "Tri", signature: signature<(freq?: Value) => Audio>(), controls: { freq: FREQ }, output: port.audio(), positional: ["freq"] },
    ({ freq: f }: Params) => kernel("std:tri", { freq: freq(f) }),
  ),
};

/** White noise. The same seed always gives the same noise. */
export const noise = component(
  {
    id: "noise",
    label: "Noise",
    signature: signature<(seed?: number) => Audio>(),
    // Fixed when the song builds (changing it rebuilds): a setting, not a knob (D3).
    settings: { seed: setting.number(1) },
    output: port.audio(),
    positional: ["seed"],
  },
  ({ seed }: Params) => kernel("std:noise", { seed }),
);

/** Attack/decay/sustain/release envelope, 0..1, driven by a gate such as `note.gate`. */
export const adsr = component(
  {
    id: "adsr",
    label: "ADSR",
    signature: signature<(gate: Value) => Control<"unit01">>(),
    inputs: { gate: port.control("unit01") },
    controls: {
      attack: control.secs(0.005).range(0.001, 10).log(),
      decay: control.secs(0.2).range(0.001, 10).log(),
      sustain: control.frac(0.7).range(0, 1),
      release: control.secs(0.2).range(0.001, 10).log(),
    },
    output: port.control("unit01"),
  },
  (gate: Value, { attack, decay, sustain, release }: Params) => kernel("std:adsr", { gate, attack, decay, sustain, release }),
);

/** Envelope follower: how loud `x` is right now, 0..1. */
export const follow = component(
  {
    id: "follow",
    label: "Follow",
    signature: signature<(x: Audio) => Control<"unit01">>(),
    // It only listens: muting what it follows must not change what it reports (PKG-13).
    inputs: { in: port.audio({ sidechain: true }) },
    controls: { attack: control.secs(0.002).range(0.0001, 1).log(), release: control.secs(0.12).range(0.001, 5).log() },
    output: port.control("unit01"),
  },
  (x: Audio, { attack, release }: Params) => kernel("std:follow", { in: x, attack, release }),
);

// The kernel reads both every sample without smoothing, so a step would click (PKG-14).
const CUTOFF = control.hz(1000).range(20, 20000).log().smooth();
const Q = control.scalar(0.707).range(0.3, 20).log().smooth();

/** State-variable filter. */
export const svf = {
  lowpass: component(
    {
      id: "svf-lowpass",
      label: "SVF lowpass",
      signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
      inputs: { in: port.audio() },
      controls: { cutoff: CUTOFF, q: Q },
      output: port.audio(),
      positional: ["cutoff", "q"],
    },
    (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("lp"),
  ),
  bandpass: component(
    {
      id: "svf-bandpass",
      label: "SVF bandpass",
      signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
      inputs: { in: port.audio() },
      controls: { cutoff: CUTOFF, q: Q },
      output: port.audio(),
      positional: ["cutoff", "q"],
    },
    (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("bp"),
  ),
  highpass: component(
    {
      id: "svf-highpass",
      label: "SVF highpass",
      signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
      inputs: { in: port.audio() },
      controls: { cutoff: CUTOFF, q: Q },
      output: port.audio(),
      positional: ["cutoff", "q"],
    },
    (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("hp"),
  ),
};

/** Multiply audio by a gain (an envelope, a mask, a number). */
export const vca = component(
  {
    id: "vca",
    label: "VCA",
    signature: signature<(x: Audio, gain?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { gain: control.linear(1).range(0, 2).smooth() },
    output: port.audio(),
    positional: ["gain"],
  },
  (x: Audio, { gain }: Params) => kernel("std:scale", { in: x, by: gain }),
);

/** Gain in dB. */
export const gain = component(
  {
    id: "gain",
    label: "Gain",
    signature: signature<(x: Audio, db?: Value) => Audio>(),
    inputs: { in: port.audio() },
    // The kernel smooths it itself.
    controls: { db: control.db(0).range(-60, 12) },
    output: port.audio(),
    positional: ["db"],
    editor: { category: "Utility", description: "Volume: louder or quieter, in dB", insert: { db: 0 } },
  },
  (x: Audio, { db }: Params) => kernel("std:gain", { in: x, db }),
);

/** Place it from -1 (left) to 1 (right): a mono sound is panned into stereo, a stereo one balanced. */
export const pan = component(
  {
    id: "pan",
    label: "Pan",
    signature: signature<(x: Audio, position?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { position: control.bipolar(0).range(-1, 1).smooth() },
    output: port.audio(),
    positional: ["position"],
    editor: { category: "Utility", description: "Places a mono sound left or right", insert: { position: 0 } },
  },
  (x: Audio, { position }: Params) => kernel("std:pan", { in: x, pan: position }),
);

// ---------------------------------------------------------------- voices

/** What a voice plays: the note's pitch (in semitones, the pitch wheel in it), gate and velocity, and the mod wheel, as `poly` gives them. */
export type Voice = { pitch: Control<"semitones">; gate: Control<"unit01">; velocity: Control<"unit01">; modWheel: Control<"unit01"> };

/**
 * Play `voice` one note at a time: `poly` in `"legato"` mode, without retriggering. The
 * latest note wins. One that starts while another is still held slides to its pitch over
 * `glide` (seconds) and keeps the envelopes going (legato); notes that only touch start
 * again, their envelopes from where they are. When the note sounding ends while older ones
 * are still held, the newest of those sounds again, sliding back to it (an 808's slide up
 * and back). Notes that start together count as one, the last of them.
 */
export function mono(notes: Events, voice: (note: Voice) => Audio, opts: { glide?: Value } = {}): Audio {
  return poly(notes, voice, { mode: "legato", retrigger: false, glide: opts.glide ?? 0 });
}
