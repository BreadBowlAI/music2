// The calls of @m2/mixing (PACK-26), checked as the app's code editor checks songs: each tool as the
// editor adds it (its `insert`) and bare, a band's menu and switch by their names and values, a
// gate keyed by another sound, and its pictures' dots (CARD-110). A line that should fail carries `@ts-expect-error`, so a type that
// stops refusing it fails the check too.

import { component, control, db, hz, khz, ms, port, type Audio } from "@m2/core";
import { deesser, eq, gate, multiband, pump, transient } from "@m2/mixing";
import { curvePicture } from "std/pictures";
import { band, bandPicture, dip, dipPicture, hit, hitPicture, levelsPicture, split, threshold } from "../../../../packs/@m2/mixing/src/pictures";

declare const x: Audio, kick: Audio, snare: Audio;

export const mixing: Audio[] = [
  // As + Effect adds each, and bare.
  eq(x),
  gate(x, { threshold: -40 }),
  transient(x, { attack: 6, sustain: 0 }),
  deesser(x, { freq: 6000, threshold: -30, amount: 6 }),
  multiband(x),
  pump(x, { depth: -8, shape: 0.5 }),
  pump(x),
  // Pump's rate is a menu of note lengths; its depth takes dB.
  pump(x, { rate: "1/8", depth: db(-12), shape: 1 }),
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
  // @ts-expect-error: Pump's rate is one of its note lengths.
  pump(x, { rate: "1/3" }),
  // @ts-expect-error: Pump takes no key: it follows the song's beats.
  pump(x, kick),
];

// Its pictures' dots name the card's own values in the unit their axis draws (CARD-62, CARD-110),
// as std's do: each mistake is marked where it is named.

export const dots = [
  component(
    { id: "gate-in-hz", inputs: { in: port.audio() }, controls: { threshold: control.hz(100) }, output: port.audio(), editor: { views: { levels: levelsPicture() } } },
    (x) => x,
    {
      levels: {
        // @ts-expect-error a gate's threshold that is a frequency, on an axis of levels
        draw: (v, p) => threshold(p, v, { threshold: "threshold" }),
        words: () => "a gate",
      },
    },
  ),
  component(
    {
      id: "attack-a-time",
      inputs: { in: port.audio() },
      controls: { attack: control.secs(0.01), sustain: control.db(0) },
      output: port.audio(),
      editor: { views: { hit: hitPicture() } },
    },
    (x) => x,
    {
      hit: {
        // @ts-expect-error a transient's attack in seconds, moved up and down in dB
        draw: (v, p) => hit(p, v, { attack: "attack", sustain: "sustain" }),
        words: () => "a hit",
      },
    },
  ),
  component(
    {
      id: "band-in-db",
      inputs: { in: port.audio() },
      controls: { freq: control.db(0), threshold: control.db(-30) },
      output: port.audio(),
      editor: { views: { band: bandPicture() } },
    },
    (x) => x,
    {
      band: {
        // @ts-expect-error a de-esser's frequency that is a level, moved across frequency
        draw: (v, p) => band(p, v, { freq: "freq", threshold: "threshold" }),
        words: () => "a band",
      },
    },
  ),
  component(
    {
      id: "gain-in-hz",
      inputs: { in: port.audio() },
      controls: { lowMid: control.hz(200), midHigh: control.hz(2500), lowGain: control.hz(1), midGain: control.db(0), highGain: control.db(0) },
      output: port.audio(),
      editor: { views: { split: curvePicture() }, groups: { Low: ["lowGain"], Mid: ["midGain"], High: ["highGain"] } },
    },
    (x) => x,
    {
      split: {
        // @ts-expect-error a band's gain that is a frequency
        draw: (v, p) => split(p, v, { lowMid: "lowMid", midHigh: "midHigh", gains: ["lowGain", "midGain", "highGain"], tabs: ["Low", "Mid", "High"] }),
        words: () => "a split",
      },
    },
  ),
  component(
    {
      id: "split-tab-missing",
      inputs: { in: port.audio() },
      controls: { lowMid: control.hz(200), midHigh: control.hz(2500), lowGain: control.db(0), midGain: control.db(0), highGain: control.db(0) },
      output: port.audio(),
      editor: { views: { split: curvePicture() }, groups: { Low: ["lowGain"], Mid: ["midGain"], High: ["highGain"] } },
    },
    (x) => x,
    {
      split: {
        // @ts-expect-error a dot that shows a tab the card hasn't got
        draw: (v, p) => split(p, v, { lowMid: "lowMid", midHigh: "midHigh", gains: ["lowGain", "midGain", "highGain"], tabs: ["Low", "Middle", "High"] }),
        words: () => "a split",
      },
    },
  ),
  component(
    {
      id: "depth-a-level",
      inputs: { in: port.audio() },
      controls: { depth: control.frac(0.5), shape: control.frac(0.5) },
      output: port.audio(),
      editor: { views: { dip: dipPicture() } },
    },
    (x) => x,
    {
      dip: {
        // @ts-expect-error Pump's depth from 0 to 1, moved up and down in dB
        draw: (v, p) => dip(p, v, { depth: "depth", shape: "shape", rate: "rate" }),
        words: () => "a dip",
      },
    },
  ),
];
