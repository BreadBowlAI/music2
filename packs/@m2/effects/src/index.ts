// @m2/effects: effects that colour a sound (PACK-15): echoes and space, modulation, drive and crush,
// and width. The effects mixing needs (EQ, compression, limiting, ducking) are std's, in std/fx.
//
// Each declares an `editor.category`, so the editor offers it after any audio and writes its
// `insert` when it is added. A control is `.smooth()` when its kernel reads it every sample without
// smoothing it, so a step would click (PKG-14); the others' kernels smooth their controls
// themselves, or read them once per block. Chorus and drive play std's kernel of their name (as
// @m2/synths does); the others play the pack's own (`@m2/effects:delay` …, built from kernels-src/,
// PACK-21).
//
// Each card's face is its picture (CARD-117 to CARD-124, ./pictures), drawn from the effect's own
// sums: the delay's echoes, the reverb's tail, a second of the chorus's, the phaser's and the
// tremolo's motion, drive's curve, a crushed sine, the width; and beside it every knob it has, so
// none has groups: a tab would only show the face again (CARD-110).
//
// Chorus, phaser, tremolo and bitcrush follow the song: each passes `time.playing` to its
// kernel, so its wobble (or the bitcrusher's sample clock) moves only while the song plays.
// Stopped, they wait; Play carries on. Played after a stop, a song sounds as its export does.

import { component, control, kernel, port, signature, time, type Audio, type Params, type Value } from "@m2/core";
import { hzWords } from "std/pictures";
import {
  chorusMotion,
  crush,
  crushPicture,
  drive as driveCurve,
  driven,
  DRIVE_AT,
  echoes,
  echoesPicture,
  echoesWords,
  field,
  fieldPicture,
  fieldWords,
  figures,
  motionPicture,
  phaserMotion,
  shapePicture,
  tail,
  tailPicture,
  tailWords,
  tremoloMotion,
} from "./pictures";

// ---------------------------------------------------------------- time

/** Feedback delay. `time` up to 2 seconds; `mix` 0 is dry, 1 is only echoes. */
export const delay = component(
  {
    id: "delay",
    label: "Delay",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      time: control.secs(0.25).range(0.001, 2).log().smooth(),
      feedback: control.frac(0.3).range(0, 0.95).smooth(),
      mix: control.frac(0.3).range(0, 1).smooth(),
    },
    output: port.audio(),
    editor: {
      category: "Delay & Reverb",
      description: "Echoes that repeat and fade away",
      insert: { time: 0.25, feedback: 0.3, mix: 0.3 },
      // Its echoes, from its own sum (CARD-117).
      views: { echoes: echoesPicture() },
      face: ["echoes", "time", "feedback", "mix"],
    },
  },
  (x: Audio, { time, feedback, mix }: Params) => kernel("@m2/effects:delay", { in: x, time, feedback, mix }),
  {
    echoes: {
      draw: (v, p) => echoes(p, v, { time: "time", feedback: "feedback", mix: "mix" }),
      words: (v) => echoesWords(v, { time: "time", feedback: "feedback", mix: "mix" }),
    },
  },
);

/**
 * Reverb: the sound of a room around `x`. `size` 0..1 goes from a small room to a huge
 * hall, `damping` 0..1 from a bright tail to a dark one, and `mix` 0..1 from dry to only
 * reverb. A stereo sound gets a stereo tail; a mono one stays mono, so it can still be panned.
 */
export const reverb = component(
  {
    id: "reverb",
    label: "Reverb",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      size: control.frac(0.6).range(0, 1),
      damping: control.frac(0.5).range(0, 1),
      mix: control.frac(0.25).range(0, 1),
    },
    output: port.audio(),
    editor: {
      category: "Delay & Reverb",
      description: "Space: the sound of a room or a hall around it",
      insert: { size: 0.6, damping: 0.5, mix: 0.25 },
      // Its tail as it fades, from its own sums (CARD-118).
      views: { tail: tailPicture() },
      face: ["tail", "size", "damping", "mix"],
    },
  },
  (x: Audio, { size, damping, mix }: Params) => kernel("@m2/effects:reverb", { in: x, size, damping, mix }),
  { tail: { draw: (v, p) => tail(p, v, { size: "size", damping: "damping" }), words: (v) => tailWords(v, { size: "size", damping: "damping" }) } },
);

// ---------------------------------------------------------------- modulation

/** Chorus: thickens `x` with slowly drifting copies (spread left and right if `x` is stereo). `rate` in Hz; `depth` and `mix` 0..1. */
export const chorus = component(
  {
    id: "chorus",
    label: "Chorus",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      rate: control.hz(0.8).range(0.05, 20).log(),
      depth: control.frac(0.5).range(0, 1),
      mix: control.frac(0.5).range(0, 1),
    },
    output: port.audio(),
    editor: {
      category: "Modulation",
      description: "Thicker and wider, with a gentle shimmer",
      insert: { rate: 0.8, depth: 0.5, mix: 0.5 },
      // A second of its motion: how late each of its two copies is (CARD-119).
      views: { motion: motionPicture("seconds", 0, 0.015) },
      face: ["motion", "rate", "depth", "mix"],
    },
  },
  (x: Audio, { rate, depth, mix }: Params) => kernel("std:chorus", { in: x, rate, depth, mix, playing: time.playing }),
  {
    motion: {
      draw: (v, p) => chorusMotion(p, v, { rate: "rate", depth: "depth" }),
      words: (v) =>
        v.rate === null || v.depth === null
          ? "Two copies drifting in time: its rate or depth is set by a cable or the code"
          : `Two copies of the sound, 8 to ${figures(8 + 6 * v.depth)} ms late, drifting apart and back ${figures(v.rate)} times a second`,
    },
  },
);

/** Phaser: sweeping notches. `rate` in Hz; `depth`, `feedback` (a sharper sound) and `mix` 0..1, where 0.5 sweeps deepest. */
export const phaser = component(
  {
    id: "phaser",
    label: "Phaser",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      rate: control.hz(0.4).range(0.05, 20).log(),
      depth: control.frac(0.7).range(0, 1),
      feedback: control.frac(0.5).range(0, 1),
      mix: control.frac(0.5).range(0, 1),
    },
    output: port.audio(),
    editor: {
      category: "Modulation",
      description: "A swirling, sweeping whoosh",
      insert: { rate: 0.4, depth: 0.7, feedback: 0.5, mix: 0.5 },
      // A second of its motion: where its notches are (CARD-120).
      views: { motion: motionPicture("hz", 20, 20000, true) },
      face: ["motion", "rate", "depth", "feedback", "mix"],
    },
  },
  (x: Audio, { rate, depth, feedback, mix }: Params) => kernel("@m2/effects:phaser", { in: x, rate, depth, feedback, mix, playing: time.playing }),
  {
    motion: {
      draw: (v, p) => phaserMotion(p, v, { rate: "rate", depth: "depth" }),
      words: (v) =>
        v.rate === null || v.depth === null
          ? "Three notches sweeping: its rate or depth is set by a cable or the code"
          : `Three notches sweep round ${hzWords(640)}, up to ${figures(2.5 * v.depth)} octaves either way, ${figures(v.rate)} times a second`,
    },
  },
);

/** Tremolo: the volume pulses `rate` times a second, dipping by `depth` (1 is down to silence). */
export const tremolo = component(
  {
    id: "tremolo",
    label: "Tremolo",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { rate: control.hz(5).range(0.05, 20).log(), depth: control.frac(0.5).range(0, 1) },
    output: port.audio(),
    editor: {
      category: "Modulation",
      description: "Pulses the volume up and down",
      insert: { rate: 5, depth: 0.5 },
      // A second of its motion: how loud it leaves the sound (CARD-121).
      views: { motion: motionPicture("unit01", 0, 1) },
      face: ["motion", "rate", "depth"],
    },
  },
  (x: Audio, { rate, depth }: Params) => kernel("@m2/effects:tremolo", { in: x, rate, depth, playing: time.playing }),
  {
    motion: {
      draw: (v, p) => tremoloMotion(p, v, { rate: "rate", depth: "depth" }),
      words: (v) =>
        v.rate === null || v.depth === null
          ? "The level pulsing: its rate or depth is set by a cable or the code"
          : `The level dips to ${figures(1 - v.depth)} and back, ${figures(v.rate)} times a second`,
    },
  },
);

// ---------------------------------------------------------------- colour

/** Soft saturation; `amount` is dB of drive. */
export const drive = component(
  {
    id: "drive",
    label: "Drive",
    signature: signature<(x: Audio, amount?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { amount: control.db(6).range(0, 36).smooth() },
    output: port.audio(),
    positional: ["amount"],
    editor: {
      category: "Distortion",
      description: "Warm saturation, from gentle grit to crunch",
      insert: { amount: 6 },
      // Its curve, from its own sum (CARD-122).
      views: { curve: shapePicture() },
      face: ["curve", "amount"],
    },
  },
  (x: Audio, { amount }: Params) => kernel("std:drive", { in: x, amount }),
  {
    curve: {
      draw: (v, p) => driveCurve(p, v, { amount: "amount" }),
      words: (v) =>
        v.amount === null
          ? "Rounds off loud peaks: its amount is set by a cable or the code"
          : `Rounds off loud peaks: a sound at ${DRIVE_AT} comes out at ${figures(driven(DRIVE_AT, v.amount))}, one at 1 still at 1. Drag the dot for more`,
    },
  },
);

/** Bitcrusher: `bits` of resolution (16 is clean, 4 is harsh) at a lower sample `rate` in Hz, like an old sampler. `mix` 0..1. */
export const bitcrush = component(
  {
    id: "bitcrush",
    label: "Bitcrush",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      bits: control.scalar(8).range(1, 16),
      rate: control.hz(12000).range(100, 48000).log(),
      mix: control.frac(1).range(0, 1),
    },
    output: port.audio(),
    editor: {
      category: "Distortion",
      description: "Lo-fi digital crunch, like an old sampler",
      insert: { bits: 8, rate: 12000, mix: 1 },
      // A sine, crushed, from its own sum (CARD-123).
      views: { crush: crushPicture() },
      face: ["crush", "bits", "rate", "mix"],
    },
  },
  (x: Audio, { bits, rate, mix }: Params) => kernel("@m2/effects:bitcrush", { in: x, bits, rate, mix, playing: time.playing }),
  {
    crush: {
      draw: (v, p) => crush(p, v, { bits: "bits", rate: "rate", mix: "mix" }),
      words: (v) =>
        v.bits === null || v.rate === null
          ? "A sine, crushed: its bits or rate is set by a cable or the code"
          : `A 1 kHz sine, rounded to ${figures(v.bits)} bits and held ${figures(v.rate / 1000)} thousand times a second`,
    },
  },
);

// ---------------------------------------------------------------- stereo

/** Stereo width: 0 is mono, 1 leaves it alone, 2 is twice as wide. The result is stereo: a mono sound gets width too, and still sums cleanly back to mono. */
export const widen = component(
  {
    id: "widen",
    label: "Widen",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { width: control.scalar(1.5).range(0, 2) },
    output: port.audio(),
    editor: {
      category: "Utility",
      description: "Makes the stereo image wider, or narrower toward mono",
      insert: { width: 1.5 },
      // How wide it leaves the sound (CARD-124).
      views: { field: fieldPicture() },
      face: ["field", "width"],
    },
  },
  (x: Audio, { width }: Params) => kernel("@m2/effects:widen", { in: x, width }),
  { field: { draw: (v, p) => field(p, v, { width: "width" }), words: (v) => fieldWords(v, { width: "width" }) } },
);
