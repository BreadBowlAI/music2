// The calls of @m2/mixing (PACK-26), checked as the app's code editor checks songs: each tool as the
// editor adds it (its `insert`) and bare, a band's menu and switch by their names and values, and a
// gate keyed by another sound. A line that should fail carries `@ts-expect-error`, so a type that
// stops refusing it fails the check too.

import { db, hz, khz, ms, type Audio } from "@m2/core";
import { deesser, eq, gate, multiband, transient } from "@m2/mixing";

declare const x: Audio, kick: Audio, snare: Audio;

export const mixing: Audio[] = [
  // As + Effect adds each, and bare.
  eq(x),
  gate(x, { threshold: -40 }),
  transient(x, { attack: 6, sustain: 0 }),
  deesser(x, { freq: 6000, threshold: -30, amount: 6 }),
  multiband(x),
  // A band's shape is a menu, its on a switch; its numbers take units.
  eq(x, { shape3: "high shelf", freq3: khz(4), gain3: db(3), q3: 0.7, on1: true, shape1: "low cut", freq1: hz(80) }),
  // A gate keyed by another track's sound, with its values.
  gate(x, kick, { threshold: db(-30), attack: ms(0.5), hold: ms(40), release: ms(120), range: db(-60) }),
  gate(x, kick),
  transient(x, { attack: -6, sustain: db(9) }),
  multiband(x, { lowMid: hz(150), midHigh: khz(3), lowRatio: 4, lowThreshold: db(-30), highGain: db(2), midAttack: ms(5) }),
  // @ts-expect-error: a shape is one of the band's names.
  eq(x, { shape2: "notch" }),
  // @ts-expect-error: on is true or false.
  eq(x, { on4: 1 }),
  // @ts-expect-error: there are eight bands.
  eq(x, { gain9: 3 }),
  // @ts-expect-error: a gate listens to one key.
  gate(x, kick, snare),
  // @ts-expect-error: a gate's key is a sound, not a number.
  gate(x, 0.5),
];
