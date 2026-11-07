// The calls of @m2/notes (PACK-29 to PACK-33), checked as the app's code editor checks songs: each
// note effect bare and with its values, a menu by its names, a whole number as a knob, notes in and
// notes out, so one goes before an instrument and another note effect. A line that should fail
// carries `@ts-expect-error`, so a type that stops refusing it fails the check too.

import { semis, type Audio, type Ctx, type Events } from "@m2/core";
import { arp, chord, echo, scale } from "@m2/notes";
import { synth } from "@m2/synths";

declare const x: Audio;

export function notes(ctx: Ctx) {
  const held = ctx.lane.notes("chords");
  // Each bare, and as the design's calls write them.
  const played: Events[] = [
    arp(held),
    arp(held, { rate: "1/16", octaves: "2" }),
    arp(held, { style: "updown", rate: "1/8T", gate: 1.5, hold: true, swing: 0.6 }),
    chord(held),
    chord(held, { shape: "7th", inKey: false, inversion: 2 }),
    echo(held),
    echo(held, { time: "1/8", repeats: 3, fall: 0.3, pitch: semis(12) }),
    scale(held),
    scale(held, { shift: -2 }),
    // A chain: notes out are notes in.
    arp(chord(scale(held)), { gate: ctx.lane.control("arp-gate", "scalar", 0.8) }),
    arp(held, { bypass: true }),
  ];
  // Before an instrument, which plays their notes.
  const heard: Audio[] = [synth.pluck(arp(held, { rate: "1/16" })), synth.pad(chord(scale(held), { shape: "7th" }))];
  // @ts-expect-error a note effect takes notes, not a sound
  arp(x);
  // @ts-expect-error a rate is one of the menu's names
  arp(held, { rate: "1/12" });
  // @ts-expect-error octaves are a menu's names, not a number
  arp(held, { octaves: 2 });
  // @ts-expect-error a shape is one of Chord's
  chord(held, { shape: "9th" });
  // @ts-expect-error In key is true or false
  chord(held, { inKey: 1 });
  // @ts-expect-error an echo's time is a menu's name
  echo(held, { time: 0.5 });
  // @ts-expect-error the notes a note effect gives are notes, not a sound
  const loud: Audio = arp(held);
  return { played, heard, loud };
}
