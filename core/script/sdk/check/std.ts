// std's calls, checked as the app's code editor checks songs (PKG-1, the step 5 design's D15):
// what the fixtures and templates write, every effect as the editor adds it (its `insert`), and
// controls left out written back in, positionally or in the options object (D5). A line that
// should fail carries `@ts-expect-error`, so a type that stops refusing it fails the check too.

import { db, hz, khz, ms, secs, semis, type Audio, type Control, type Events, type Signal } from "@m2/core";
import { choose, lfo, when } from "std/control";
import { adsr, follow, gain, noise, osc, pan, svf, vca } from "std/dsp";
import { bandpass, highpass, lowpass, notch } from "std/filter";
import { bitcrush, chorus, compress, delay, drive, duck, eq3, limit, phaser, reverb, tremolo, widen } from "std/fx";
import { bass, hat, kick, pad, pluck } from "std/instruments";
import { add, blend, mtof, range, scale, sum, xrange } from "std/math";
import { master, mixer } from "std/mix";

declare const x: Audio, notes: Events, beats: Control<"beats">, l: Control<"unit01">, pitch: Control<"semitones">, s: Signal;

// The generic helpers keep their kinds; the others say what they return.
export const audio: Audio[] = [add(x, x), scale(x, 0.5), scale(x), blend(x, x, l), sum(x, x, x), when(l, x), choose(l, x, x)];
export const pitched: Control<"semitones"> = add(pitch, -12);
export const hertz: Control<"hz"> = mtof(60);
export const unit01: Control<"unit01">[] = [lfo(beats), adsr(l), follow(x)];
export const mapped: Control[] = [range(l, 1, 2), xrange(l, hz(200), khz(2.5))];

// What the fixtures and templates write.
export const written = [
  master(mixer({ kick: x, hats: { in: x, db: db(-6), pan: 0.3 }, bass: duck(x, l, { depth: db(-9) }) })),
  master(compress(mixer({ clap: { in: x, db: db(2) } }), { threshold: db(-12), ratio: 2, attack: 0.02, release: 0.2, makeup: db(3) })),
  mixer({}),
  xrange(lfo(beats, { period: 0.5 }), hz(200), khz(2.5)),
  bass(notes, { cutoff: blend(s, s, l) }),
  bass(notes, { cutoff: hz(420), drive: 3 }),
  hat(notes),
  hat(notes, { decay: ms(40) }),
  pluck(notes),
  pluck(notes, { brightness: hz(4200) }),
  pad(notes, { cutoff: hz(1600) }),
  kick(notes),
  follow(x, { release: ms(150) }),
  adsr(l, { attack: ms(1), decay: ms(160), sustain: 0, release: ms(80) }),
  vca(svf.bandpass(noise(3), hz(1500), 1.2), scale(l, scale(l, 2))),
  reverb(chorus(widen(x, { width: 1.6 }), { rate: 0.6, depth: 0.4, mix: 0.4 }), { size: 0.75, damping: 0.5, mix: 0.3 }),
  delay(x, { time: 0.388, feedback: 0.3, mix: 0.25 }),
  reverb(x, { size: 0.6, mix: 0.2 }),
];

// Every effect as the editor adds it, then left bare.
export const effects = [
  lowpass(x, 1000),
  highpass(x, 200),
  bandpass(x, 1000),
  notch(x, 1000),
  limit(x, -1),
  drive(x, 6),
  delay(x, { time: 0.25, feedback: 0.3, mix: 0.3 }),
  gain(x, 0),
  pan(x, 0),
  reverb(x, { size: 0.6, damping: 0.5, mix: 0.25 }),
  chorus(x, { rate: 0.8, depth: 0.5, mix: 0.5 }),
  phaser(x, { rate: 0.4, depth: 0.7, feedback: 0.5, mix: 0.5 }),
  tremolo(x, { rate: 5, depth: 0.5 }),
  compress(x, { threshold: -18, ratio: 3, attack: 0.01, release: 0.15, makeup: 4 }),
  eq3(x, { low: 0, mid: 0, high: 0 }),
  bitcrush(x, { bits: 8, rate: 12000, mix: 1 }),
  widen(x, { width: 1.5 }),
  ...[lowpass(x), highpass(x), bandpass(x), notch(x), limit(x), drive(x), delay(x), gain(x), pan(x)],
  ...[reverb(x), chorus(x), phaser(x), tremolo(x), compress(x), eq3(x), bitcrush(x), widen(x)],
];

// A control left out, written in: after the positional arguments when that is contiguous,
// else in an options object, or a cable's name in its place.
export const inserted = [
  lowpass(x, 1000, 0.8),
  lowpass(x, { q: 0.8 }),
  lowpass(x, 1000, { q: 0.8 }),
  lowpass(x, undefined, 0.8),
  lowpass(x, s, l),
  svf.lowpass(x, 800),
  svf.highpass(x, { q: 2 }),
  osc.square(440, 0.3),
  osc.square({ width: 0.3 }),
  osc.sine(),
  osc.saw(semis(60)),
  osc.tri(pitch),
  noise(),
  noise(7),
  delay(x, { time: secs(0.5), mix: 0.3 }),
  duck(x, l, { depth: -6 }),
  bass(notes, { drive: 6, voices: 2 }),
  kick(notes, { tune: 50, decay: s }),
  master(x, { ceiling: -3 }),
  lfo(beats, { period: 2, phase: 0.25, shape: "tri" }),
  adsr(l, { attack: 0.01 }),
  mixer({ kick: x, hats: { in: x, db: -6, pan: 0.3 }, keys: { in: x, pan: l } }),
  eq3(x, { lowFreq: 150, highFreq: hz(5000) }),
];

// Mistakes the editor flags, as the build does (D1).
// @ts-expect-error a misspelt option
delay(x, { tme: 0.5 });
// @ts-expect-error an options object, then a positional argument
lowpass(x, { q: 1 }, 0.8);
// @ts-expect-error the audio left out
reverb();
// @ts-expect-error a shape lfo doesn't have
lfo(beats, { shape: "ramp" });
// @ts-expect-error a mixer entry with a field it doesn't have
mixer({ hats: { in: x, gain: -6 } });
// @ts-expect-error a string where a value goes
gain(x, "-6");
// @ts-expect-error a signal for a setting, which shapes the graph
bass(notes, { voices: l });
// @ts-expect-error duck's side input left out
duck(x, { depth: -6 });
// @ts-expect-error mtof's pitch left out
mtof();
