// std/mix: summing tracks into the master.

import { busOf, component, control, each, feed, kernel, port, setting, signature, type Audio, type Params, type Signal, type Value } from "@m2/core";
import { gain, pan } from "./dsp";
import { limit } from "./fx";
import { sum } from "./math";

/** A send to a bus channel: its level (dB), after the channel's pan and fader, or before them with `pre: true`. */
export type Send = Value | { db?: Value; pre?: boolean };

/**
 * A mixer channel: its audio, or its audio with a gain (dB), a pan, sends to bus channels, and
 * `to`, the bus channel it goes to instead of the master.
 */
export type Channel = Audio | { in: Audio; db?: Value; pan?: Value; sends?: Record<string, Send>; to?: string };

/** A channel as the body gets it. */
type Strip = { in: Audio; db: Signal; pan?: Signal; sends: Record<string, { db: Signal; pre: boolean }>; to?: string };

/**
 * Sum channels, each with a gain (dB, 0 when left out) and an optional pan (a stereo channel is
 * balanced). −96 dB or lower is silence. No channels is silence.
 * Each channel's key names it, so `hats.db` is the hats fader (PKG-6).
 *
 * A channel that reads a bus (`verb: verb.out`) is that bus's channel: the mixer feeds the bus
 * what the other channels send it (`sends: { verb: db(-12) }`, after the fader unless `pre: true`)
 * and the channels that go to it (`to: "drums"`, instead of the master), summed in key order, or
 * silence (RENDER-34). A bus that would feed itself is refused, naming the loop.
 */
export const mixer = component(
  {
    id: "mixer",
    label: "Mixer",
    signature: signature<(channels: Record<string, Channel>) => Audio>(),
    controls: {
      channels: each({
        in: port.audio(),
        // Every channel has a gain, its fader: at 0 dB it is `x × 1.0`, which changes no bit.
        db: control.db(0).range(-60, 12),
        // Optional: a channel gets a panner only when it says so, as a mono sound through a
        // centred panner isn't bit for bit the same (PKG-5).
        pan: control.bipolar(0).range(-1, 1).optional().smooth(),
        // Sends to bus channels, by their keys: a level each, after the fader unless `pre`.
        sends: each({ db: control.db(-12).range(-60, 6), pre: setting.bool(false) }),
        // The bus channel this channel goes to instead of the master.
        to: setting.key().optional(),
      }),
    },
    output: port.audio(),
    positional: ["channels"],
  },
  function mixer({ channels }: Params): Audio {
    const chans = channels as Record<string, Strip>;
    const keys = Object.keys(chans);
    // The bus channels, by key: each reads a bus as it is.
    const buses = new Map<string, object>();
    for (const k of keys) {
      const b = busOf(chans[k].in);
      if (b !== undefined) buses.set(k, b);
    }
    // Where each channel's sound goes besides the master: its `to`, then its sends.
    const goes = (k: string) => [...(chans[k].to === undefined ? [] : [chans[k].to!]), ...Object.keys(chans[k].sends)];
    for (const k of keys) {
      for (const b of goes(k)) {
        if (!buses.has(b)) {
          const how = b === chans[k].to ? "goes to" : "sends to";
          throw new Error(`mixer: ${k} ${how} "${b}", which isn't a bus channel of this mixer. A bus channel reads the bus as it is: ${b}: ${b}.out`);
        }
      }
    }
    const loop = loopFrom([...buses.keys()], goes);
    if (loop) {
      throw new Error(`mixer: ${loop[0]} feeds itself: ${loop.join(" → ")}. A bus can't feed itself. Take away one of these sends or outputs.`);
    }
    // In the caller's key order, as written.
    const strips = keys.map((k) => gain(chans[k].pan === undefined ? chans[k].in : pan(chans[k].in, chans[k].pan!), chans[k].db));
    // A song with no tracks yet (the Empty template) still plays: a sine at 0 Hz, times 0.
    const silence = (): Audio => kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 });
    if (strips.length === 0) return silence();
    if (buses.size > 0) {
      const into = new Map<string, Audio[]>([...buses.keys()].map((b) => [b, []]));
      keys.forEach((k, i) => {
        const c = chans[k];
        if (c.to !== undefined) into.get(c.to)!.push(strips[i]);
        for (const [b, s] of Object.entries(c.sends)) into.get(b)!.push(gain(s.pre ? c.in : strips[i], s.db));
      });
      for (const [b, xs] of into) feed(buses.get(b)!, xs.length === 0 ? silence() : sum(...xs));
    }
    return sum(...strips.filter((_, i) => chans[keys[i]].to === undefined));
  },
);

/** A way from one of `starts` back to itself along `goes`, if there is one: the keys on it, the first again at the end. */
function loopFrom(starts: string[], goes: (k: string) => string[]): string[] | undefined {
  for (const s of starts) {
    const path = [s];
    const seen = new Set<string>();
    const walk = (k: string): boolean => {
      for (const n of goes(k)) {
        path.push(n);
        if (n === s) return true;
        if (!seen.has(n)) {
          seen.add(n);
          if (walk(n)) return true;
        }
        path.pop();
      }
      return false;
    };
    if (walk(s)) return path;
  }
  return undefined;
}

/** The end of the chain: a gain (dB), then a limiter so the song never clips. −96 dB or lower is silence. */
export const master = component(
  {
    id: "master",
    label: "Master",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { db: control.db(0).range(-60, 12), ceiling: control.db(-1).range(-24, 0) },
    output: port.audio(),
  },
  // Stereo from the start, sample for sample: a mono mix whose first channel is panned turns
  // stereo, and the gain and limiter keep going rather than start again (design-8b §5.1).
  (x: Audio, { db, ceiling }: Params) => limit(gain(kernel("std:stereo", { in: x }), db), ceiling),
);
