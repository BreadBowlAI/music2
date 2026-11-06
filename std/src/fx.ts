// std/fx: the effects mixing needs (PKG-17): EQ, compression, a limiter and ducking. The effects
// that colour a sound (delay, reverb, chorus, drive …) are the pack @m2/effects.
//
// Each listed effect declares an `editor.category`, so the editor offers it after any audio and
// writes its `insert` when it is added. A control is `.smooth()` when its kernel reads it every
// sample without smoothing it, so a step would click (PKG-14); the others' kernels smooth their
// controls themselves, or read them once per block.

//
// Their cards' faces (CARD-92 to CARD-95): eq3's curve, measured from its own sound, with a dot for
// each band; the compressor's transfer curve, drawn from its own sum since a compressor's sound
// changes with its level (D13), with the live level riding it, and how far it turns down as it
// plays, from its own meter (D8); the limiter's and the duck's how far, the same way.

import { component, control, kernel, port, signature, type Audio, type Params, type Value } from "@m2/core";
import { curvePicture, dbWords, hzWords, reduction, reductionPicture, transfer, transferPicture, transferWords } from "./pictures";

/**
 * Turn `x` down by `depth` dB while `side` is high: `duck(bass.out, kick.env)`, or
 * `follow(kick.out)` for a track with no `env`. With no `side` (as + Effect adds it, until its card
 * picks what it listens to) it plays `x` as it is, bit for bit (PKG-19).
 */
export const duck = component(
  {
    id: "duck",
    label: "Duck",
    signature: signature<(x: Audio, side?: Value) => Audio>(),
    // `side` only listens: muting the track it comes from must not stop the ducking (PKG-13). A
    // call may leave it out (PKG-18): its card's Listens to picks it (CARD-35).
    inputs: { in: port.audio(), side: port.control("unit01", { sidechain: true }).optional() },
    controls: { depth: control.db(-8).range(-60, 0) },
    output: port.audio(),
    editor: {
      category: "Dynamics",
      description: "Turns the sound down while another track plays: pick it in Listens to",
      insert: { depth: -8 },
      views: { down: reductionPicture() },
      face: ["down", "depth"],
    },
  },
  // With no side the kernel's side is 0, so its gain is exactly 1.
  (x: Audio, side: Value | undefined, { depth }: Params) => kernel("std:duck", { in: x, side: side ?? 0, depth }),
  {
    down: {
      draw: (v, p) => reduction(p, { most: v.depth === null ? null : -v.depth }),
      words: (v) => `Turns down by up to ${v.depth === null ? "a depth set elsewhere" : dbWords(-v.depth)} while what it listens to plays`,
    },
  },
);

/** Peak limiter with a hard ceiling in dB. */
export const limit = component(
  {
    id: "limit",
    label: "Limit",
    signature: signature<(x: Audio, ceiling?: Value) => Audio>(),
    inputs: { in: port.audio() },
    // Not smooth: the kernel reads it once per block, which a one-block ramp can't reach.
    controls: { ceiling: control.db(-1).range(-24, 0) },
    output: port.audio(),
    positional: ["ceiling"],
    editor: { category: "Dynamics", description: "Stops peaks from clipping", insert: { ceiling: -1 }, views: { down: reductionPicture({ most: 12 }) }, face: ["down", "ceiling"] },
  },
  (x: Audio, { ceiling }: Params) => kernel("std:limit", { in: x, ceiling }),
  {
    down: {
      draw: (v, p) => reduction(p),
      words: (v) => `Turns peaks down so they stay under ${v.ceiling === null ? "a ceiling set elsewhere" : dbWords(v.ceiling)}`,
    },
  },
);

// ---------------------------------------------------------------- dynamics and tone

/**
 * Compressor: above `threshold` (dB) the level rises only 1/`ratio` as fast, so loud and
 * quiet moments even out. `attack` and `release` (seconds) are how fast it grabs and lets
 * go; `makeup` (dB) brings the level back up. It hears its own sound, or a key given after it:
 * `compress(bass.out, kick.out)` turns the bass down only while the kick plays (PKG-19).
 */
export const compress = component(
  {
    id: "compress",
    label: "Compress",
    signature: signature<(x: Audio, key?: Audio) => Audio>(),
    // `key` only listens (PKG-13), and a call may leave it out (PKG-18): its card's Listens to
    // picks it (CARD-35).
    inputs: { in: port.audio(), key: port.audio({ sidechain: true }).optional() },
    controls: {
      threshold: control.db(-18).range(-60, 0),
      ratio: control.scalar(3).range(1, 20).log(),
      attack: control.secs(0.01).range(0.0001, 1).log(),
      release: control.secs(0.15).range(0.001, 5).log(),
      makeup: control.db(0).range(0, 24),
    },
    output: port.audio(),
    // Added with some makeup gain, though left out it is 0 dB.
    editor: {
      category: "Dynamics",
      description: "Evens out loud and quiet moments so the sound sits steady",
      insert: { threshold: -18, ratio: 3, attack: 0.01, release: 0.15, makeup: 4 },
      views: { transfer: transferPicture(), down: reductionPicture() },
      face: ["transfer", "down", "threshold", "ratio", "attack", "release", "makeup"],
    },
  },
  // With no key its detector hears `x` itself, as it did before it took one.
  (x: Audio, key: Audio | undefined, { threshold, ratio, attack, release, makeup }: Params) =>
    kernel("std:compress", { in: x, threshold, ratio, attack, release, makeup, key: key ?? x }),
  {
    transfer: {
      draw: (v, p) => transfer(p, v, { threshold: "threshold", ratio: "ratio", makeup: "makeup" }),
      words: (v) => transferWords(v, { threshold: "threshold", ratio: "ratio", makeup: "makeup" }),
    },
    down: { draw: (v, p) => reduction(p), words: () => "How far it turned the sound down in the last 3 seconds" },
  },
);

/** Three-band EQ: `low`, `mid` and `high` in dB (0 leaves the sound alone), around `lowFreq`, `midFreq` and `highFreq` in Hz. */
export const eq3 = component(
  {
    id: "eq3",
    label: "EQ3",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      low: control.db(0).range(-60, 24),
      mid: control.db(0).range(-60, 24),
      high: control.db(0).range(-60, 24),
      lowFreq: control.hz(200).range(20, 20000).log().label("low frequency"),
      midFreq: control.hz(1000).range(20, 20000).log().label("mid frequency"),
      highFreq: control.hz(4000).range(20, 20000).log().label("high frequency"),
    },
    output: port.audio(),
    editor: {
      category: "EQ",
      description: "Bass, middle and treble, each up or down in dB",
      insert: { low: 0, mid: 0, high: 0 },
      views: { curve: curvePicture() },
      face: ["curve", "low", "mid", "high", "lowFreq", "midFreq", "highFreq"],
    },
  },
  (x: Audio, { low, mid, high, lowFreq, midFreq, highFreq }: Params) =>
    kernel("std:eq3", { in: x, low, mid, high, low_freq: lowFreq, mid_freq: midFreq, high_freq: highFreq }),
  {
    // Each band's dot at its frequency and its gain, kept on the picture (a cut to −60 dB rests at
    // its foot): one colour, numbered (D14).
    curve: {
      draw: (v, p) => {
        const on = (db: number | null) => (db === null ? null : Math.min(24, Math.max(-24, db)));
        return [
          p.response({ name: "curve" }),
          p.handle(v.lowFreq, on(v.low), { x: "lowFreq", y: "low", mark: "1" }),
          p.handle(v.midFreq, on(v.mid), { x: "midFreq", y: "mid", mark: "2" }),
          p.handle(v.highFreq, on(v.high), { x: "highFreq", y: "high", mark: "3" }),
        ];
      },
      words: (v) => {
        const band = (name: string, db: number | null, hz: number | null) =>
          `${name} ${db === null ? "set elsewhere" : dbWords(db)} at ${hz === null ? "a frequency set elsewhere" : hzWords(hz)}`;
        return `${band("Low", v.low, v.lowFreq)}; ${band("mid", v.mid, v.midFreq)}; ${band("high", v.high, v.highFreq)}`;
      },
    },
  },
);
