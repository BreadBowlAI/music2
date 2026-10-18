// std/sampler: plays audio files by note.
//
// One engine, the `std:sampler` kernel, in up to `voices` voices. Give it one file, `sample`, in
// the song's `assets/` folder: every note plays it, sped up or slowed down by how far the note is
// from `root` (an octave up plays it twice as fast). Or give it `zones`, a file for each root note:
// each covers from its root up to the next zone's root (the lowest down to note 0 too), and a note
// plays the zone it falls in. `zones` wins when both are written.
//
// `mode: "oneshot"` (the default) plays the whole file however short the note, as drum hits and
// chops want; `"gate"` stops with the note, fading out over `release`. With no file it plays
// silence: drop a WAV on its card in the app to give it one.

import { component, control, kernel, poly, port, setting, signature, type Audio, type Events, type Params } from "@m2/core";

/** The kernel's settings for `sample` at `root`, or for `zones`: its files, and a root, a lowest and a highest note for each. */
function files(sample: string | undefined, root: number, zones: Readonly<Record<number, string>>): { assets: string[]; values: number[] } {
  const roots = Object.keys(zones)
    .map(Number)
    .sort((a, b) => a - b);
  if (roots.length > 0) {
    const values = roots.flatMap((r, i) => [r, i === 0 ? 0 : r, i + 1 < roots.length ? roots[i + 1] - 1 : 127]);
    return { assets: roots.map((r) => zones[r]), values };
  }
  return sample === undefined ? { assets: [], values: [] } : { assets: [sample], values: [root, 0, 127] };
}

/** A sampler: one WAV file on every note, pitched by the note, or a file for each range of notes. */
export const sampler = component(
  {
    id: "sampler",
    label: "Sampler",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: {
      tune: control.semis(0).range(-24, 24),
      level: control.db(0).range(-24, 12),
      attack: control.secs(0).range(0, 2),
      release: control.secs(0.05).range(0, 5),
    },
    settings: {
      sample: setting.asset().optional(),
      root: setting.int(60),
      zones: setting.keymap(),
      mode: setting.choice(["oneshot", "gate"], "oneshot"),
      voices: setting.int(8),
    },
    output: port.audio(),
    editor: { category: "Sampler", description: "Plays a WAV file on every note, pitched by the note: drop a WAV on its card" },
  },
  (notes: Events, p: Params): Audio => {
    const settings = files(p.sample, p.root, p.zones);
    const mode = p.mode === "gate" ? 1 : 0;
    return poly(
      notes,
      (n) => kernel("std:sampler", { pitch: n.pitch, gate: n.gate, velocity: n.velocity, tune: p.tune, level: p.level, attack: p.attack, release: p.release, mode }, settings),
      { voices: p.voices },
    );
  },
);
