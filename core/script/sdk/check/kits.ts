// The kits a drum names (KIT-3), typed against this very descriptor, as the code editor types them:
// each track's values are the instrument's own controls', taken as a call takes them (a name, true
// or false, a plain number or a quantity of the control's unit), and only a drum (category "Drums")
// names kits. A line that should fail carries `@ts-expect-error`, so a type that stops refusing it
// fails the check too. `kits_tests.rs` builds the same mistakes, and the build refuses each of them
// again, even through `as any`.

import { component, control, db, hz, kernel, ms, port, semis, setting, type Audio, type Events } from "@m2/core";
import { drums } from "@m2/drums";

const controls = {
  drum: control.choice(["kick", "snare", "hats"], "kick").label("sound"),
  tune: control.semis(0).range(-24, 24),
  decay: control.ms(200).range(10, 2000),
  soft: control.bool(false),
};
const body = (notes: Events): Audio => kernel("test:drum", { notes }) as Audio;

/** A drum that names two kits: a choice by its name, numbers plain and as quantities, a switch, a fader. */
export const drum = component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    settings: { voices: setting.int(6) },
    output: port.audio(),
    editor: {
      category: "Drums",
      kits: {
        "Basic kit": {
          db: -2,
          tracks: [{ name: "Kick", drum: "kick" }, { name: "Snare", drum: "snare", tune: semis(-2), decay: ms(80) }, { name: "Hats", drum: "hats", soft: true, tune: 2 }],
        },
        Two: { db: db(-4), tracks: [{ name: "Kick" }] },
        Three: { tracks: [{ name: "Kick", decay: 0.08 }] },
      },
    },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error a sound the drum doesn't have
    editor: { category: "Drums", kits: { "Basic kit": { tracks: [{ name: "Kick", drum: "kik" }] } } },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error a choice is written by its name, not its place
    editor: { category: "Drums", kits: { "Basic kit": { tracks: [{ name: "Kick", drum: 0 }] } } },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error a switch is true or false
    editor: { category: "Drums", kits: { "Basic kit": { tracks: [{ name: "Kick", soft: 1 }] } } },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error a quantity of another unit than the control's
    editor: { category: "Drums", kits: { "Basic kit": { tracks: [{ name: "Kick", tune: hz(2) }] } } },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    settings: { voices: setting.int(6) },
    output: port.audio(),
    // @ts-expect-error a control the drum doesn't have (a setting isn't a kit's value)
    editor: { category: "Drums", kits: { "Basic kit": { tracks: [{ name: "Kick", voices: 2 }] } } },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error every track has a name
    editor: { category: "Drums", kits: { "Basic kit": { tracks: [{ drum: "kick" }] } } },
  },
  body,
);

component(
  {
    id: "drum",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error a kit's fader is in dB
    editor: { category: "Drums", kits: { "Basic kit": { db: ms(2), tracks: [{ name: "Kick" }] } } },
  },
  body,
);

component(
  {
    id: "keys",
    inputs: { notes: port.notes() },
    controls,
    output: port.audio(),
    // @ts-expect-error only a drum (category "Drums") names kits
    editor: { category: "Keys", kits: { Pair: { tracks: [{ name: "Low", tune: -12 }] } } },
  },
  body,
);

// @m2/drums' own kits type-check as any pack's do, and its Drum plays as before.
export const played: Audio[] = [drums.drum(null as unknown as Events, { drum: "snare" })];
