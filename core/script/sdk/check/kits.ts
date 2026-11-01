// The kits an instrument names (KIT-3), typed against this very descriptor, as the code editor
// types them: each track's values are the instrument's own controls', as a call writes them. A line
// that should fail carries `@ts-expect-error`, so a type that stops refusing it fails the check too.
// `kits_tests.rs` builds the same mistakes, and the build refuses each of them again, even through
// `as any`.

import { component, control, kernel, port, setting, type Audio, type Events } from "@m2/core";
import { drums } from "@m2/drums";

const controls = {
  drum: control.choice(["kick", "snare", "hats"], "kick").label("sound"),
  tune: control.semis(0).range(-24, 24),
  soft: control.bool(false),
};
const body = (notes: Events): Audio => kernel("test:drum", { notes }) as Audio;

/** A drum that names two kits: a choice by its name, a number, a switch. */
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
        "Basic kit": [{ name: "Kick", drum: "kick" }, { name: "Snare", drum: "snare", tune: -2 }, { name: "Hats", drum: "hats", soft: true }],
        Two: [{ name: "Kick" }],
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
    editor: { category: "Drums", kits: { "Basic kit": [{ name: "Kick", drum: "kik" }] } },
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
    editor: { category: "Drums", kits: { "Basic kit": [{ name: "Kick", drum: 0 }] } },
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
    editor: { category: "Drums", kits: { "Basic kit": [{ name: "Kick", soft: 1 }] } },
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
    editor: { category: "Drums", kits: { "Basic kit": [{ name: "Kick", voices: 2 }] } },
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
    editor: { category: "Drums", kits: { "Basic kit": [{ drum: "kick" }] } },
  },
  body,
);

// @m2/drums' own kits type-check as any pack's do, and its Drum plays as before.
export const played: Audio[] = [drums.drum(null as unknown as Events, { drum: "snare" })];
