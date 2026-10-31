// std's calls, checked as the app's code editor checks songs (PKG-1, the step 5 design's D15):
// what the fixtures and templates write, every effect as the editor adds it (its `insert`), and
// controls left out written back in, positionally or in the options object (D5). A line that
// should fail carries `@ts-expect-error`, so a type that stops refusing it fails the check too.

import { db, hz, khz, kernel, ms, poly, secs, semis, type Audio, type Control, type Events, type Region, type Signal } from "@m2/core";
import { choose, lfo, when } from "std/control";
import { adsr, follow, gain, noise, osc, pan, svf, vca } from "std/dsp";
import { bandpass, highpass, lowpass, notch } from "std/filter";
import { bitcrush, chorus, compress, delay, drive, duck, eq3, limit, phaser, reverb, tremolo, widen } from "std/fx";
import { drums } from "std/drums";
import { fm } from "std/fm";
import { bass, hat, kick, pad, pluck } from "std/instruments";
import { add, blend, mtof, range, scale, sum, xrange } from "std/math";
import { master, mixer } from "std/mix";
import { sampler } from "std/sampler";
import { mono, synth } from "std/synth";

declare const x: Audio, notes: Events, beats: Control<"beats">, l: Control<"unit01">, pitch: Control<"semitones">, s: Signal;

// Regions made by a helper, or kept in a `const`, as a sample map of many files is written: a pair
// is then a list of two numbers, not a pair, and still type-checks. A trigger kept in a `const` is
// a string, unless the const says it holds regions.
const take = (file: string, key: number, of: number, position: number) => ({ file, root: key, keys: [key - 1, key + 1], round_robin: [position, of] });
const piano = [
  { file: "C4-soft.wav", velocity: [1, 80], loop: [100, 4000] },
  { file: "C4-hard.wav", velocity: [81, 127] },
];
const keyUps: Region[] = [{ file: "C4-up.wav", trigger: "release", keys: [0, 127] }];

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

// The instruments of std/synth, std/fm and std/drums: bare, with knobs and settings written,
// and mono() around a voice of one's own.
export const instruments: Audio[] = [
  synth.custom(notes),
  synth.custom(notes, { unison: 7, detune: 0.4, filterEnv: 2, filterDecay: ms(250), voices: 1, glide: ms(80) }),
  synth.supersaw(notes, { detune: 0.5, cutoff: khz(12), release: secs(0.5) }),
  synth.lead(notes, { glide: ms(80), vibrato: semis(0.2) }),
  synth.lead(notes, { voices: 4 }),
  synth.pad(notes, { attack: secs(1), cutoff: l }),
  synth.pluck(notes, { brightness: hz(6000), decay: 0.3 }),
  synth.sub(notes, { drop: semis(12), drive: db(9), glide: ms(50), decay: secs(1.5) }),
  synth.bass(notes, { cutoff: hz(500), bite: 2, decay: ms(150), glide: ms(30), velSens: 0.6 }),
  synth.strings(notes, { ensemble: 0.7 }),
  fm.custom(notes, { carrierA: 2, modulatorA: 3.5, amountA: 3, feedback: 0.3, drop: semis(5), voices: 6 }),
  fm.custom(notes, { voices: 1 }),
  fm.keys(notes, { tone: 0.7, tine: 0.5, velSens: 1 }),
  fm.bell(notes, { decay: secs(6) }),
  fm.metal(notes, { drop: semis(12), feedback: 0.6, decay: ms(300) }),
  drums.kit(notes, { kickTune: hz(52), hatDecay: ms(30), drive: db(6), hatLevel: db(4), kickLevel: db(-2) }),
  drums.kick(notes, { tune: 50, punch: 0.8 }),
  drums.snare(notes, { snap: 0.8 }),
  drums.clap(notes),
  drums.hats(notes, { decay: ms(40), openDecay: ms(600) }),
  drums.tom(notes, { tune: hz(110) }),
  drums.rim(notes),
  drums.cowbell(notes),
  drums.cymbal(notes, { decay: secs(2.5) }),
  drums.shaker(notes),
  mono(notes, (n) => vca(osc.saw(n.pitch), adsr(n.gate)), { glide: ms(60) }),
  mono(notes, (n) => osc.square(n.pitch)),
  sampler(notes, { sample: "pluck.wav", mode: "gate", velocity: 0.5 }),
  sampler(notes, { zones: { 36: "kick.wav", 38: "snare.wav" } }),
  // Regions (PKG-15): only `file` must be given; pairs are written as two numbers.
  sampler(notes, {
    regions: [
      { file: "C4-soft.wav", keys: [58, 62], velocity: [1, 80], round_robin: [1, 2] },
      { file: "C4-up.wav", trigger: "release", volume: -12, pan: -0.2, tune: 5, offset: 10 },
      { file: "pad.wav", root: 55, loop: [2400, 5399] },
    ],
  }),
  sampler(notes, { regions: [take("C4-1.wav", 60, 2, 1), take("C4-2.wav", 60, 2, 2), ...piano, ...keyUps] }),
  sampler(notes, { regions: piano }),
  // A turn for each key (PKG-16 §3, std 0.6.0), and a voice that reads its note's key count.
  sampler(notes, { regions: piano, turns: "key" }),
  poly(notes, (n) => kernel("std:regions", { pitch: n.pitch, gate: n.gate, count: n.keyCount })),
  // @ts-expect-error: turns are "lane" or "key".
  sampler(notes, { regions: piano, turns: "note" }),
  // @ts-expect-error: a region needs a file.
  sampler(notes, { regions: [{ root: 60 }] }),
  // @ts-expect-error: a trigger is "attack" or "release".
  sampler(notes, { regions: [{ file: "a.wav", trigger: "up" }] }),
  // @ts-expect-error: a region has no field `lokey`.
  sampler(notes, { regions: [{ file: "a.wav", lokey: 3 }] }),
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

// Every effect turned off (Next D1): `bypass` in its options, a new options object when it had none.
export const bypassed = [
  reverb(x, { size: 0.8, bypass: true }),
  reverb(x, { bypass: true }),
  lowpass(x, 1000, { bypass: true }),
  gain(x, 0, { bypass: true }),
  gain(x, { bypass: false }),
  limit(x, -1, { bypass: true }),
  duck(x, l, { depth: db(-9), bypass: true }),
  master(x, { bypass: true }),
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
// @ts-expect-error glide belongs to the instruments that can play one note at a time
synth.supersaw(notes, { glide: ms(80) });
// @ts-expect-error a signal for the unison count, a setting
synth.custom(notes, { unison: l });
// @ts-expect-error the kit's knobs are named by their drum
drums.kit(notes, { tune: 52 });
// @ts-expect-error the 808's drop was called punch once
synth.sub(notes, { punch: 7 });
// @ts-expect-error an instrument can't be bypassed: it takes notes
kick(notes, { bypass: true });
// @ts-expect-error a source can't be bypassed
osc.saw(110, { bypass: true });
// @ts-expect-error a control's call can't be bypassed
lfo(beats, { bypass: true });
// @ts-expect-error bypass is true or false
reverb(x, { bypass: 1 });
