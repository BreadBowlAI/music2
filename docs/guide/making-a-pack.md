# Making a pack

**In short**
- A pack is a folder: `m2pkg.toml` says what it is, `src/` holds its instruments and effects in TypeScript, `presets/` its sounds, `samples/` its recordings.
- `m2 pack new <folder> --name @you/name` makes one to start from. `m2 pack check` says what to fix; `m2 pack install` puts it where songs find it.
- To try it in a song while you work on it, point the song at its folder: `"@you/name" = { path = "../name" }`.

## The folder

```
felt-piano/
  m2pkg.toml            what the pack is
  README.md  LICENSE
  src/index.ts          its instruments and effects, each a component(…)
  src/keys.ts           more of them, imported as "@alice/felt-piano/keys"
  presets/<id>/*.json   its sounds: presets/felt-piano/Soft.json
  samples/              recordings its code plays
```

A pack holds only `.ts`, `.wasm`, `.toml`, `.json`, `.md`, `.txt` and `.wav` files, and `LICENSE`.

## m2pkg.toml

```toml
[package]
name = "@alice/felt-piano"     # @owner/name: lowercase letters, digits and -
version = "1.2.0"              # a new version for every change
license = "CC-BY-4.0"          # one that lets anyone share it and use it in songs they sell
description = "A soft felt piano, recorded close"
authors = ["Alice Example"]
repository = "https://github.com/alice/felt-piano"
tags = ["keys", "piano"]

[dependencies]                 # only these two
"@m2/core" = "1"
std = "0.4"
```

Every key is checked: one music2 doesn't know is an error, so a typo never goes unseen.

## An instrument

An instrument is a `component(…)` whose first input is notes and whose output is sound; an effect's first input is sound. Give each a label and a category: the browser groups by it.

```ts
import { component, control, file, port, signature, type Audio, type Events, type Params } from "@m2/core";
import { sampler } from "std/sampler";
import { synth } from "std/synth";

export const feltPiano = component(
  {
    id: "felt-piano",                      // its id in songs: @alice/felt-piano:felt-piano
    label: "Felt piano",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000) },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck" },
  },
  (notes: Events, { brightness }: Params): Audio => synth.pluck(notes, { brightness }),
);

// A recording of the pack's own: file(import.meta, …) names it, "@alice/felt-piano:samples/C4.wav".
export const feltSample = component(
  {
    id: "felt-sample",
    label: "Felt sample",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    output: port.audio(),
    editor: { category: "Keys", description: "One recorded note, pitched" },
  },
  (notes: Events): Audio => sampler(notes, { sample: file(import.meta, "samples/C4.wav"), mode: "gate" }),
);
```

A recording is named by the pack and its path in the pack: `"@alice/felt-piano:samples/C4.wav"`. It must be inside the pack, spelled exactly as the file is named.

## Sounds

A sound is an instrument's settings under a name, one file in `presets/<id>/`:

```json
{
  "m2preset": 1,
  "name": "Soft",
  "sound": "@alice/felt-piano:felt-piano",
  "values": {
    "brightness": 900
  }
}
```

It shows in the instrument's **Presets**, under music2's own.

## Trying it, checking it, installing it

| Step | Command |
|---|---|
| Start one | `m2 pack new felt-piano --name @alice/felt-piano` |
| Try it in a song | in the song's `music2.toml`: `"@alice/felt-piano" = { path = "../felt-piano" }` |
| Check it | `m2 pack check felt-piano`: every problem in words, exit 1 if any |
| List its files | `m2 pack build felt-piano` writes `m2pack.json`: every file, its size and its fingerprint |
| Install it | `m2 pack install felt-piano` |

`m2 pack check` checks what the pack registry will: the strict `m2pkg.toml`, the file types, a licence, every instrument and effect has a category and builds in a song, every kernel loads, every sound loads on its instrument, and every recording its code names is in the pack.

A version is never changed once installed: to change the pack, raise its version.
