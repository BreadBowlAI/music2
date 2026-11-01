# Making a pack

**In short**
- A pack is a folder: `m2pkg.toml` says what it is, `src/` holds its instruments and effects in TypeScript, `presets/` its sounds, `samples/` its recordings.
- `m2 pack new <folder> --name @you/name` makes one to start from. `m2 pack check` says what to fix; `m2 pack install` puts it where songs find it.
- An instrument made of recordings needs no code: write it as data, `instruments/<id>.toml`, or make the whole pack from an SFZ instrument with `m2 pack import-sfz`.
- To try it in a song while you work on it, point the song at its folder: `"@you/name" = { path = "../name" }`.

## The folder

```
felt-piano/
  m2pkg.toml            what the pack is
  README.md  LICENSE
  src/index.ts          its instruments and effects, each a component(…)
  src/keys.ts           more of them, imported as "@alice/felt-piano/keys"
  instruments/*.toml    instruments written as data, imported as "@alice/felt-piano/instruments"
  presets/<id>/*.json   its sounds: presets/felt-piano/Soft.json
  samples/              recordings its code plays
  kernels/*.wasm        sound code of its own, if any (most packs have none)
  kernels-src/          that code's Rust source: no part of the pack
```

A pack holds only `.ts`, `.wasm`, `.toml`, `.json`, `.md`, `.txt`, `.wav` and `.flac` files (in any capitals: `.WAV` is a WAV), and `LICENSE`. Its `kernels-src/` folder, the source of [kernels of its own](#kernels-of-its-own), is no part of it: music2 leaves it out of the pack's files, its fingerprint and its install. Recordings and samples may be WAV or FLAC (FLAC: the same sound, about half the size; 8, 16 or 24 bits).

**How much audio a pack may hold.** music2 holds an instrument's recordings whole in memory while a song plays it. So a pack is held to what its audio files' headers say (a *sample* here is one number of sound: one channel at one moment, so a second of stereo at 48 kHz is 96,000):

| | The most | About |
|---|---|---|
| One recording | 134,217,728 samples | 23 minutes of stereo at 48 kHz |
| All the pack's WAV and FLAC files together | 268,435,456 samples | 46 minutes of stereo at 48 kHz, 1 GB as music2 plays it |

`m2 pack check` says when a pack holds more, and which file. A small grand piano (200 recordings of a few seconds) uses less than half. Bigger instruments wait for music2 to play recordings from the disk as they sound. It holds no link (a symbolic link or shortcut): copy the file in instead.

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
"@m2/core" = "2"               # this SDK: 2.0 or newer, but below 3
std = "0.4"                    # the std you made it with, or any newer one
```

Every key is checked: one music2 doesn't know is an error, so a typo never goes unseen.

`std` is the std you made the pack with: a newer music2's std, which only adds, still runs it. `@m2/core` keeps its major version: `"2"` is any 2.x, and `"2.1"` any 2.x from 2.1. Ask for the version your pack's code needs:

| Your code uses | Ask for | Why |
|---|---|---|
| nothing added since 2.0 | `"2"` | a later minor, `"2.3"`, would shut out the music2 versions between 2.0 and 2.3 |
| a setting's label (new in 2.1) | `"2.1"`, as `@m2/synths` and `@m2/wavetable` do | a music2 with 2.0 then says in words that your pack needs 2.1. Asked for as `"2"`, it would try your pack and stop in its code |

### music2's promise to your pack

`@m2/core` (the code a pack is written with) and the way a pack's own kernels talk to music2 (kernel ABI v1) are what your pack is made against. From `@m2/core` 2.0 they only grow: a change that could break a pack's code comes only as a new major version, `@m2/core` 3.

| You see | It means |
|---|---|
| Your pack, made for `@m2/core` 2, in a newer music2 that has 2.5 | It runs as it did. |
| `@a/x, which this song uses, can't be used: it is made for @m2/core 1, and this app has @m2/core 2.1.0 …` (`m2 pack check`: `@a/x 1.3.0 is made for …`) | The pack was made before the promise. Get a version made for 2, or ask its author for one. |
| `… is made for @m2/core 3, newer than this app's 2.1.0` | Update music2. |

Songs get no such promise before music2 1.0: a new music2 may ask a song to change (it says how). Packs get this one, because other people's songs play them.

## An instrument

An instrument is a `component(…)` whose first input is notes and whose output is sound; an effect's first input is sound. Give each a label and a category: the browser groups by it. A component only code should play says `editor: { unlisted: true }` instead of a category: no list offers it. `m2 pack check` names one that says neither, as one you forgot. Build a voice from std's blocks (`std/dsp`, `std/math`), as below: the code editor checks every call as you type. Call std's components, not its kernels by id: a std kernel only one of music2's packs plays may move into that pack in a later std (as `std:bitcrush` did), and a pack that calls it by its id then can't play until it is changed.

```ts
import { component, control, file, hz, ms, poly, port, secs, signature, type Audio, type Events, type Params } from "@m2/core";
import { adsr, osc, svf, vca } from "std/dsp";
import { scale, xrange } from "std/math";
import { sampler } from "std/sampler";

export const feltPiano = component(
  {
    id: "felt-piano",                      // its id in songs: @alice/felt-piano:felt-piano
    label: "Felt piano",
    signature: signature<(notes: Events) => Audio>(),
    inputs: { notes: port.notes() },
    controls: { brightness: control.hz(1800).range(200, 20000).log().smooth() },
    output: port.audio(),
    editor: { category: "Keys", description: "A soft pluck" },
  },
  // One voice a note, from std's blocks: a saw, a lowpass that closes as the note fades, a volume.
  // A pack builds on std only: never on another pack, `@m2/synths` included.
  (notes: Events, { brightness }: Params): Audio =>
    poly(
      notes,
      (n) => {
        const env = adsr(n.gate, { attack: ms(2), decay: secs(0.45), sustain: 0, release: secs(0.2) });
        return vca(svf.lowpass(osc.saw(n.pitch), xrange(env, hz(200), brightness), 0.7), scale(env, n.velocity));
      },
      { voices: 12 },
    ),
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

## Choices, switches, views and groups

A sound's code says what each of its values is. It never says how to draw it: the app picks a knob, a menu or a switch. Two kinds of control are not numbers:

| Declared | A song writes | The sound's code gets | Its kernel reads |
|---|---|---|---|
| `control.choice(["saw", "square", "sine"], "saw")` | `shape: "square"` | a `Choice` of those names | the option's place: 0, 1 or 2 |
| `control.bool(false)` | `sub: true` | a `Switch` | 1 for true, 0 for false |

- A **choice** is one of a few named options. Its last argument is what a song gets when it leaves the choice out. A song always writes the name, never the number: so a new version of your pack may add options, or change their order, and no song changes.
- A choice has 1 to 64 options, each named once. Each name is 1 to 24 characters on one line, with no space at either end, and none of `|`, `<` and `>`.
- A **switch** is on or off.
- Both play at once when they change, with no rebuild, and both can be automated: the lane holds names (`"square"`), or `true` and `false`, and each point holds until the next.
- The kernel that reads a choice names its options in its port, in `m2pkg.toml`: `"shape: control<choice<saw|square|sine>> = 0"`. Only a choice with exactly those names, in that order, reaches it; a number, or a choice of other names, is an error. A switch's port is `control<switch>`.
- The code editor checks all this as you type: a name the choice doesn't have, or a choice handed to a number such as a cutoff, is a red line, and the build says the same in words.

**Views** are pictures over the sound's own controls. A view holds no value; dragging it moves the controls it shows.

| View | Draws | Its names must be |
|---|---|---|
| `view.envelope("attack", "decay", "sustain", "release")` | the envelope's shape | three times (`control.secs`, `control.ms` or `control.beats`) and a level (`control.frac`, `control.linear` or `control.db`) |
| `view.filter("cutoff", "q")` | a low-pass filter's curve (12 dB per octave), worked out from the two knobs: a picture, not a measurement | a frequency (`control.hz`) and a `control.scalar` |
| `view.keys("regions")` | which recording plays on which keys, as std's Sampler plays them | a `setting.regions()` or `setting.keymap()` |
| `view.scope()` | the sound as it plays | (no names) |

**Groups** put the card in order. Each group names some of the sound's values and views, each once. Values in no group go last, under More.

```ts
import { component, control, kernel, poly, port, view, type Audio, type Events } from "@m2/core";

export const wobble = component(
  {
    id: "wobble",
    label: "Wobble bass",
    inputs: { notes: port.notes() },
    controls: {
      shape: control.choice(["saw", "square", "sine"], "saw"),
      sub: control.bool(true),
      cutoff: control.hz(800).range(40, 8000).log(),
      q: control.scalar(0.7),
      attack: control.secs(0.005), decay: control.secs(0.2), sustain: control.frac(0.6), release: control.secs(0.3),
    },
    output: port.audio(),
    editor: {
      category: "Bass",
      views: { amp: view.envelope("attack", "decay", "sustain", "release"), filter: view.filter("cutoff", "q") },
      groups: { Sound: ["shape", "sub"], Filter: ["filter", "cutoff", "q"], Loudness: ["amp"] },
    },
  },
  // A voice of the pack's own kernel, whose port is "shape: control<choice<saw|square|sine>> = 0".
  (notes: Events, c): Audio => poly(notes, (n) => kernel("@alice/wobble:voice", { pitch: n.pitch, gate: n.gate, ...c }) as Audio),
);
```

A song writes `wobble(lane.notes("bass"), { shape: "square" })`, and the code editor offers `"saw"`, `"square"` and `"sine"` as you type. A view or a group that names a value the sound doesn't have, or a view over a value of the wrong kind (a cutoff in seconds), is a red line under that view, or under `groups`, and the build refuses it in words. Until it is fixed, the code editor can't tell the types of the sound's code, so a parameter there with no type written may be marked too. On the card a choice is a menu and a switch a switch, the views are pictures whose corners move their knobs, and the groups are titles over their rows (see [The window](window.md)).

**Labels.** A card names each value by its name in the code, as words: `cutoff` is Cutoff. Where a name doesn't read as words (`subOctave`, `tableA`), give it a label, a control's or a setting's alike:

| Declared | The card, its undo step and `m2 graph` say |
|---|---|
| `subOctave: setting.int(-1).label("sub octave")` | Sub octave; "Polysynth sub octave" |
| `tableA: setting.asset("saw.wav").label("table A")` | Table A; "Wavetable table A" |
| `width: control.frac(0.5).label("pulse width")` | Pulse width |
| `voices: setting.int(8)` (no label) | Voices |

A song still writes the name: `subOctave: -2`. A label is one line of 1 to 24 characters, with no space at either end. A setting's label needs `@m2/core` 2.1.

**Kits.** An instrument can name kits: several tracks of it at once, that **+ Track ▸ Drum kit (several tracks)** makes, each going to one Drums bus ([Drum kits](sounds.md#drum-kits)). Each kit is a name and its tracks, each track a `name` and values of the instrument's own knobs and menus, as a call writes them:

```ts
editor: {
  category: "Drums",
  kits: { "Basic kit": [{ name: "Kick", drum: "kick" }, { name: "Snare", drum: "snare", tune: 2 }, { name: "Hats", drum: "hats" }] },
},
```

A value the instrument doesn't have (`drum: "kik"`, or a knob it hasn't got) is a red line under that track, and the build refuses it in words. A kit's name and a track's are one line of 1 to 24 characters. Kits need `@m2/core` 2.2: say `"@m2/core" = "2.2"` in `m2pkg.toml`.

A pack's own kernels (`[[kernel]]` in `m2pkg.toml`: WebAssembly, made with the Rust kit, byte by byte in `docs/kernel-abi.md`) may take a lane's notes: an input `notes: events<note>` gets them each block, stamped to the sample, with the lane's wheels and sustain pedal. Connect it straight to a notes lane, outside `poly`: `kernel("@alice/drums:machine", { notes: lane.notes("notes") })`, a drum machine that plays its own voices. A kernel can't play notes into an instrument yet: it takes notes, it doesn't make them.

## Kernels of its own

A *kernel* is the code that makes the sound, compiled to WebAssembly (wasm). Most packs need none: std's blocks and kernels make most sounds. A pack may bring its own, written in Rust with music2's kit, `m2-kernel`.

| Step | What to do |
|---|---|
| 1. The source | a Rust crate in the pack's folder, `kernels-src/`: a `Cargo.toml` and `src/lib.rs`. It is no part of the pack: only the wasm you build from it ships, and a build left in it changes nothing |
| 2. The kit | in its `Cargo.toml`: `[lib] crate-type = ["cdylib"]`, and under `[dependencies]` the kit from a copy of music2's source: `m2-kernel = { path = "<music2>/std/kernels-src/kernel" }`. Copy music2's `std/kernels-src/.cargo/config.toml` to `kernels-src/.cargo/config.toml`: it gives the wasm a small stack, so each use of your sound takes 1 MB less memory |
| 3. A kernel | a `struct` that implements the kit's `Kernel` (how many inputs and outputs, `new` and `process`), then `export!(crush, Crush);`. Write `abi!();` once in the crate |
| 4. Build it | `rustup target add wasm32-unknown-unknown` once, then `cargo build --release --target wasm32-unknown-unknown` in `kernels-src/`, and copy `target/wasm32-unknown-unknown/release/<crate>.wasm` (the crate's name with `_` for `-`) to `kernels/<name>.wasm` |
| 5. Say so | in `m2pkg.toml`, under `[package]`: `wasm = "kernels/<name>.wasm"`; and a `[[kernel]]` for each kernel, its ports in the order its code reads them |
| 6. Call it | in the pack's code: `kernel("@you/name:crush", { in: x, bits })` |

```toml
[[kernel]]
name = "crush"
description = "Rounds each sample to `bits`"
inputs = ["in: audio<*>", "bits: control<scalar> = 8 in 1..16"]
outputs = ["out: audio<*>"]
```

- music2's own `@m2/effects` is built this way: its bitcrusher's source is `packs/@m2/effects/kernels-src` in music2's repository (its crate's `Cargo.toml` is with std's, in `std/kernels-src/packs/@m2/effects`, so it builds to the same bytes in any folder), its `[[kernel]]` is in the pack's `m2pkg.toml`, and `src/index.ts` calls it.
- The kit's rules, byte by byte, are in [Kernel ABI v1](https://github.com/BreadBowlAI/music2/blob/main/docs/kernel-abi.md). Above all: take memory only when the kernel is made (`new`), never in `process`.
- **Sharing between voices.** Each voice of an instrument is a state of its own, made by `new`. Something every voice would work out the same (a table of waveforms, say) may be made once and kept in the module's static memory for all of them, as music2's `@m2/wavetable` keeps its tables. Then it must be made only in `new` and never changed after; be the same for the same settings, whichever voice is made first; be found by each voice through its settings every time it is needed (keep a key in the voice, never an address); and hold nothing that is one voice's own.
- Your pack's kernels run with a time limit on every call (Quick code, [below](#listing-it-in-the-registry)). music2's own packs' run without one, as std's do: their files are a version music2 ships, built from its own source.
- A wasm file keeps the paths of the files it was built from, for its error messages. To leave your computer's folder names out, add `"--remap-path-prefix=/home/you=~"` (your home folder) to the `rustflags` in that `config.toml`. Don't set `RUSTFLAGS` instead: it replaces the file's flags, the stack's too.
- A change to a kernel is a change to the pack: raise its version.

## An instrument written as data

An instrument that plays recordings needs no code. Write it as a file, `instruments/<id>.toml`: which recording plays for which notes, how hard, and when. music2 makes it into the same kind of instrument as one written in code.

```toml
# instruments/felt-piano.toml: its id is @alice/felt-piano:felt-piano
label = "Felt Piano"
category = "Keys"
description = "A soft felt piano, recorded close"
engine = "std:sampler"     # std's Sampler plays it: the only engine for now
mode = "gate"              # stop with the note ("oneshot" plays each recording to its end)
release = 0.4              # seconds to fade out when the note ends
velocity = 0.7             # how much playing harder makes it louder, 0 to 1
voices = 16                # how many notes sound at once
turns = "key"              # round robin: each key takes its own turns ("lane", the default, counts the lane's notes)

[[region]]                 # one recording
file = "samples/C4-soft.wav"
root = 60                  # the note it was recorded at (60 is middle C)
keys = [58, 62]            # the notes it plays
velocity = [1, 64]         # how hard, 1 to 127

[[region]]
file = "samples/C4-loud.wav"
root = 60
keys = [58, 62]
velocity = [65, 127]

[[region]]
file = "samples/C4-release.wav"
root = 60
keys = [58, 62]
trigger = "release"        # plays when the key is let go
volume = -20               # dB
```

| Key | What it is | Left out |
|---|---|---|
| `label`, `category` | its name, and the browser's group | must be there |
| `description`, `tags` | one line about it; words for search (kept for later) | none |
| `engine` | `"std:sampler"` | must be there |
| `tune`, `level`, `attack`, `release`, `velocity` | the Sampler's knobs where they start: semitones (-24 to 24), dB (-24 to 12), seconds (0 to 2), seconds (0 to 5), 0 to 1 | the Sampler's |
| `mode`, `voices`, `turns` | `"oneshot"` or `"gate"`; notes at once, 1 to 128; whose count a round robin follows, `"lane"` or `"key"` ([Sounds](sounds.md)) | `"oneshot"`, 8, `"lane"` |

Each `[[region]]` takes the Sampler's region fields: `file` (a path in the pack), `root`, `keys`, `velocity`, `round_robin` (`[1, 3]`: take 1 of 3, played in turn), `trigger` (`"attack"` or `"release"`), `loop` (`[start, end]` in frames, played while the note is held), `tune` (cents), `volume` (dB), `pan` (-1 to 1) and `offset` (frames). [Sounds](sounds.md) says what each does.

A song imports it from the pack's `instruments`, by its id in camelCase:

```ts
import { feltPiano } from "@alice/felt-piano/instruments";
```

So the file's name must be an id: lowercase letters, digits and `-`, 32 at most (`felt-piano.toml`). A name that is a word JavaScript keeps for itself (`new.toml`, `class.toml`) is refused, and so are two names that make one name in code (`a-b.toml` and `a--b.toml` are both `aB`).

The browser, + Track and search offer it as they offer the pack's other instruments. Every key is checked: a key music2 doesn't know is refused, naming the nearest one (`lable`: did you mean `label`?), and every mistake names the file and its line.

## Importing an SFZ instrument

SFZ is the text format of thousands of free sample instruments. `m2 pack import-sfz` makes a pack of one:

```
m2 pack import-sfz "Felt Piano.sfz" felt-piano --name @alice/felt-piano --label "Felt Piano" --category Keys
```

| It writes | From |
|---|---|
| `instruments/felt-piano.toml` | every region of the SFZ music2 can play, with `turns = "key"`: SFZ counts a round robin's turns by key |
| `samples/` | the recordings those regions play, copied |
| `m2pkg.toml` | the name you give; the licence from `--license`, or from the licence file beside the SFZ when it is clear |
| `README.md` | the credits in the SFZ's first comments, and what the import couldn't keep |
| `LICENSE` | the licence file beside the SFZ (`LICENSE`, `LICENCE` or `COPYING`), when there is one |

It reads SFZ's headers (`<control>`, `<global>`, `<master>`, `<group>`, `<region>`), `#define`, `#include`, `default_path`, `octave_offset` and `note_offset`, notes as numbers or names (`c4` is 60), and the opcodes that map to a region: `sample`, `key`, `lokey`, `hikey`, `pitch_keycenter`, `lovel`, `hivel`, `seq_length`, `seq_position`, `trigger`, `loop_mode`, `loop_start`, `loop_end`, `tune`, `transpose`, `volume`, `pan`, `offset`, `amp_veltrack`, `ampeg_attack` and `ampeg_release`. Everything else it names when it ends:

```
made @alice/felt-piano in felt-piano: felt-piano plays 208 regions from 208 recordings
left out: 88 regions play only with a controller moved (CC 64)
key switches: kept the default one's regions (c0)
```

A value it can't read leaves its region out and says so (`tune=+5c`; `pitch_keycenter=sample`, the root note stored in the WAV), rather than reading it as 0.

The instrument's `velocity`, `attack` and `release` are one value for all its regions, so the import takes the one that 9 in 10 of the regions played at a key's start agree on. When they don't agree, it keeps the Sampler's own and says so. The Sampler's `velocity` makes the level follow a straight line where SFZ's `amp_veltrack` follows a curve, so soft notes come out a little louder; the import says by how much.

What it reads, it reads only from the SFZ's own folder and the folders in it: a path that climbs out of it (`../`), a path that starts at the top of the disk (`/home/…`, `C:\…`), and a link (a shortcut to a file elsewhere) stop the import, naming the line. An SFZ from the internet can't copy your own files into a pack this way. If a library keeps its samples beside the SFZ's folder (`default_path=../Samples/`), copy the SFZ up into the folder that holds both and change that line to `default_path=Samples/`.

The licence: `--license` names it. Without it, the import takes the licence from the file beside the SFZ only when that file is clear about it: its first line is `SPDX-License-Identifier: CC-BY-4.0` (or another), or it is the whole text of a Creative Commons licence (CC0, CC BY, CC BY-SA). Many libraries put several licences in one file (the recordings under one, the SFZ under another), so otherwise the import stops and asks: read the file and give `--license`.

Recordings may be WAV or FLAC, and each is copied as it is (a FLAC stays a FLAC). A recording of another kind (OGG, say), or a FLAC that isn't 8, 16 or 24 bits, stops the import before anything is written; convert it to WAV first. A round robin with a take missing stops it too: SFZ plays silence on that turn, music2 needs every take. The pack is written beside the folder you name and checked as `m2 pack check` checks it; only a pack that passes gets the folder's name, so a failed import leaves nothing behind. Open `instruments/<id>.toml` afterwards to set what the SFZ leaves to its player, such as `level` and `release`.

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

## Samples

A pack can hold loose **samples** too: one-shots and loops a musician finds with ⌘F and drops into a song ([Samples](samples.md)). Put them under `samples/`, and list them in `samples.toml` at the pack's root, by folder or by file (paths inside `samples/`):

```toml
[folder."drums/kicks"]
instrument = "kick"
tags = ["punchy"]

[file."loops/dusty keys 90 Am.wav"]
type = "loop"
instrument = "keys"
bpm = 90
key = "Am"
tags = ["lofi"]
```

| Key | Says |
|---|---|
| `type` | `loop` or `one-shot` |
| `instrument` | one of: kick, snare, clap, hat, cymbal, tom, perc, bass, keys, guitar, vocal, strings, brass, pad, lead, synth, fx, texture, foley |
| `bpm` | a loop's tempo, 20 to 300 |
| `key` | `Am`, `F#`, `Bb minor`, `C` |
| `tags` | more words to find it by |

A file's own table wins over its folder's, and a deeper folder's over a shallower one's. What a table leaves out is guessed from the file's name (`dusty keys 90 Am.wav` says a tempo and a key); `key = "none"` or `instrument = "none"` takes back a guess that is wrong (a drum loop whose name reads as a key). What can be measured (length, channels, loudness) is never written: music2 reads it from the file. Only the files `samples.toml` names are samples; the rest of `samples/` is your instruments' recordings. A pack may hold samples alone, with no instrument. Only WAV files for now.

## Trying it, checking it, installing it

| Step | Command |
|---|---|
| Start one | `m2 pack new felt-piano --name @alice/felt-piano` |
| Or make one from an SFZ instrument | `m2 pack import-sfz "Felt Piano.sfz" felt-piano --name @alice/felt-piano` |
| Try it in a song | in the song's `music2.toml`: `"@alice/felt-piano" = { path = "../felt-piano" }` |
| Check it | `m2 pack check felt-piano`: every problem in words, exit 1 if any |
| List its files | `m2 pack build felt-piano` writes `m2pack.json`: every file, its size and its fingerprint |
| Install it | `m2 pack install felt-piano` |

`m2 pack check` checks what the pack registry will: the strict `m2pkg.toml`, the file types and no links, a licence, every instrument and effect has a category and builds in a song, every kernel loads, every sound is for an instrument of the pack and names only its controls (inside any range marked `.strict()`), every recording its code or its data names is in the pack, no `src/instruments.ts` hides its instruments written as data, every WAV under `samples/` reads, and `samples.toml` reads and names only files and folders that are there.

A version is never changed once installed: to change the pack, raise its version. While you try it from its folder (`{ path = … }`), a change you save to its code, its `m2pkg.toml` or its kernels reaches the song the next time the song builds: when one of the song's own files changes, or the song opens again.

## Listing it in the registry

The registry is the list of packs anyone can get: one small file a pack, in a GitHub repo. Its robot checks each pack, plays a short preview of each instrument and sound, and copies the files where music2 can fetch them. The registry isn't open yet: its repo and its storage are still being set up, and the app can't get packs from it yet.

| Step | Command or action |
|---|---|
| Say where its code is | in `m2pkg.toml`, under `[package]`: `repository = "https://github.com/alice/felt-piano"` |
| List it | in a clone of the registry: `m2 registry add <registry> felt-piano --source <address of its .tar.gz>`. It checks the pack, writes its `m2pack.json`, and writes `packs/@alice/felt-piano.toml` |
| Pack it up | `tar -czf felt-piano-1.2.0.tar.gz --exclude='.*' felt-piano`, and attach it to a GitHub release of your repo, at that address |
| Ask | a pull request to the registry with the one file |

What the registry asks beyond `m2 pack check`:

| Rule | |
|---|---|
| Your name | `@alice/…` is the GitHub account `alice`'s. Its repository and its `.tar.gz` are under `https://github.com/alice/` |
| A licence anyone can use | for sounds `CC0-1.0`, `CC-BY-3.0` or `CC-BY-4.0`; for code `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause` or `ISC`. Nothing "non-commercial" |
| Tags | from the registry's word list, `tags.txt` |
| Size | 10 GB at most |
| The same files | the `.tar.gz` holds exactly the files the listed hash names. A version, once listed, never changes: to change anything, raise the version and list it again |
| Plain words | every label, description, tag, author and file name is on one line, with no hidden character |
| Quick code | its preview renders: a kernel call that runs past its time limit (a kernel that never returns) refuses the version. While a song plays a call gets 100 ms, and 5 s everywhere else; the checks cost your kernels' calls about a fifth more ([the time limit](https://github.com/BreadBowlAI/music2/blob/main/docs/kernel-abi.md#the-time-limit-abi-6-abi-7)) |

A person from BreadBowlAI reads every pull request and merges it; the robot's answer on it is advice. To hide a version of yours from search, add `yanked = "why"` to it. Taking a version down for good (a licence or legal problem) is BreadBowlAI's to do.
