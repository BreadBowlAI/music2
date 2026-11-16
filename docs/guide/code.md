# How a song is written

**In short**
- A song is a folder: `music2.toml`, `song.ts` (the mix), one file per track in `tracks/`, and the data in `data/`.
- This page shows each file, the units, the SDK (`@m2/core`) and how to declare your own component.
- The app edits the code for you, one small checked change at a time.

## The folder

```
my-song/
  music2.toml          # the song's name, its format and sample rate, and the SDK and packages it uses
  music2.lock          # the exact version of each package: written by music2, never by hand
  song.ts              # the mix: which tracks play, how loud, and the master
  tracks/
    bass.ts            # one file per track: its lanes, instrument and effects
  data/
    arrangement.json   # tempo, time signature and key
    bass/              # one folder per track, named by its key
      notes.json       # one file per lane: data/<track key>/<lane key>.json
      cutoff.json      # (a key that is a Windows device name, such as aux, is written aux-)
  assets/              # audio files (WAV), never overwritten
  layout.json          # the tracks' order and colours, and markers: shown, never heard
  AGENTS.md            # for AI agents working on the song: what is here, its rules, the path of m2
  CLAUDE.md            # the same for Claude Code: one line, @AGENTS.md. The app ignores both
  .music2/             # this machine's: not part of the song, and not in git
```

`music2.toml` looks like this:

```toml
[project]
name = "My song"
format = 1
sample_rate = 48000

[dependencies]
"@m2/core" = "2"
std = "0.10"
"@m2/synths" = "2"
```

Each package the code imports from is listed: the SDK (`@m2/core`), std (the toolbox) and each pack, such as `@m2/synths`, one of the packs music2 ships ([Packs](packs.md)). `music2.lock` beside it holds the exact version of each pack.

`arrangement.json` holds `tempo` (a list of `{ "beat", "bpm" }` changes, the first at beat 0), `meter` (a list of `{ "beat", "beats", "unit" }` time signatures; empty is 4/4) and `key`, the song's key: `{ "root": "A", "scale": "minor" }`, or left out for none. A beat is a quarter note. A root is written `C`, `C#`, `Eb` and so on; a scale by its name (`major`, `minor`, `dorian`, `harmonic-minor`, …: every one is in [Clip gestures as JSON](api.md#clip-gestures-as-json)). `m2 key <song> A minor` sets it, and `m2 key <song> none` takes it out. One key is for the whole song. A key music2 doesn't know (a typo) is a warning, and the song plays as if it had no key.

`layout.json` is written the first time you move, recolour or add a track, or add a marker: `{"order": ["kick", "hats", …], "tracks": {"kick": {"colour": "rose"}, …}, "markers": [{"beat": 64, "name": "Chorus"}]}`. A marker is a name on the ruler at a beat, one a beat, its name 1 to 40 characters on one line. A colour is one of the twelve names (rose, teal, amber, indigo, green, plum, olive, blue, coral, cyan, violet, lime). A track that is missing for a while (commented out of `song.ts`) keeps its place and colour there, and keys the app doesn't know are kept as they are. If it stops being valid JSON (a git merge can leave `<<<<<<<` lines in it), the tracks show in their own order and colours, the status pill says so, and **Reset layout** there writes the default one over it (Undo brings the old text back).

Tracks, lanes and plugins are filed under their keys: short names in lowercase letters, digits and hyphens, such as `lead-vox`. A key is written in the code, so renaming a track's label never moves its data.

## A track

This is a whole track, the shape of `tracks/hats.ts`:

```ts
import { track } from "@m2/core";
import { drums } from "@m2/drums";

export default track("hats", { name: "Hats" }, ({ lane }) => drums.drum(lane.notes("hits"), { drum: "hats" }));
```

- `track("hats", { name: "Hats" }, …)` makes a track with the key `hats`, shown as Hats. Without the options, `track("hats", fn)`, it is shown by its key. Its data is filed under the key, so changing the name moves nothing; changing the key leaves its data behind until you move it (the app offers to).
- `lane.notes("hits")` is a notes lane with the key hits. Its clips are in `data/hats/hits.json`. `lane.notes("hits", { name: "Hits" })` gives it a label too.
- `drums.drum(…, { drum: "hats" })` is the Drum, an instrument from `@m2/drums`, a pack that comes with music2, playing a hi-hat. It plays those notes. The song lists the pack in `music2.toml` and locks it in `music2.lock` ([Packs](packs.md)).

## The mix

`song.ts` says what you hear. Here the bass ducks (dips in volume) each time the kick hits, which is called a sidechain. The numbers are only examples.

```ts
import { db, song } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import bass from "./tracks/bass";
import hats from "./tracks/hats";
import kick from "./tracks/kick";

export default song(() =>
  master(
    mixer({
      kick: kick.out,
      hats: { in: hats.out, db: db(-8), pan: 0.2 }, // quieter, a little to the right
      bass: duck(bass.out, kick.env, { depth: db(-10) }),
    }),
  ),
);
```

`kick.env` is how loud the kick is right now. The kick track exports it next to its sound, as `tracks/kick.ts` does in the `first-song` example ([Try it on a copy](changes.md#try-it-on-a-copy)):

```ts
import { ms, track } from "@m2/core";
import { drums } from "@m2/drums";
import { follow } from "std/dsp";

export default track("kick", { name: "Kick" }, ({ lane }) => {
  const out = drums.drum(lane.notes("hits"));
  return { out, env: follow(out, { release: ms(150) }) };
});
```

## Buses, sends and groups

**In short:** a bus is a track that plays what other tracks send it, such as one reverb they share. All the routing is written on the mixer's channels in `song.ts`.

| Word | Meaning |
|---|---|
| **bus** | a track with no notes or clips: its sound comes from other tracks. A shared reverb, a shared delay, a group |
| **send** | a copy of a track's sound going to a bus, at its own level. The track still plays on its own channel too |
| **after the fader** | the send follows the track's fader and pan, so turning the track down turns its send down too (the default) |
| **before the fader** | `pre: true`: the send ignores the track's fader and pan |
| **group** | a bus that tracks go *to* instead of the master: one fader, and one place for effects, for all the drums |

A bus is a file in `tracks/`, like a track, made with `bus(key, { name }, fn)` from `@m2/core`. `fn` gets `input`, the sum of what reaches the bus, and returns its sound. A shared reverb, `tracks/verb.ts`:

```ts
import { bus } from "@m2/core";
import { reverb } from "@m2/effects";

export default bus("verb", { name: "Reverb" }, ({ input }) => reverb(input, { mix: 1 }));
```

`mix: 1` makes the reverb all room and none of the sound itself: each track's own channel plays that. A group, `tracks/drums.ts`, plays what reaches it as it is, through a `gain` at 0 dB (from `std/dsp`): `bus("drums", { name: "Drums" }, ({ input }) => gain(input))`.

Then `song.ts` imports both, as it imports a track, and gives each a channel in the mixer:

```ts
import drums from "./tracks/drums"; // beside the tracks' imports
import verb from "./tracks/verb";

export default song(() =>
  master(
    mixer({
      kick: { in: kick.out, to: "drums" },                                 // into the Drums group
      clap: { in: clap.out, db: db(2), to: "drums" },
      drums: drums.out,                                                    // the group's own fader
      bass: { in: bass.out, db: db(-10), sends: { verb: db(-18) } },        // after the fader
      chords: { in: chords.out, sends: { verb: { db: db(-12), pre: true } } }, // before the fader
      verb: verb.out,                                                      // the reverb's own fader
    }),
  ),
);
```

| On a channel | Means |
|---|---|
| `sends: { verb: db(-18) }` | send to the bus whose channel is `verb`, 18 dB down, after the fader |
| `sends: { verb: { db: db(-12), pre: true } }` | send to `verb`, 12 dB down, before the fader |
| `to: "drums"` | go to the bus whose channel is `drums`, instead of the master |
| `verb: verb.out` | the bus's own channel, written just so. A send or a `to` naming a bus with no channel is refused |

- A send and `to` name the bus by its channel's key in the mixer. A channel can have several sends, and a bus can send to another bus.
- A send's level is a knob, as a fader is: `m2 controls` lists it ([How a knob is named](changes.md#how-a-knob-is-named)).
- A bus that would feed itself (verb → delay → verb) is refused, in words that name the loop.
- Muting a track silences its sends and its part of the group. Soloing a track keeps the buses it feeds playing.
- In the app, **Mix** (⌥3) ▸ **Add bus** writes the same, and each strip's sends and output change them ([The window](window.md#the-dock-below-the-timeline)). No `m2` command adds a bus or a send: write the files with `m2 write` and `m2 edit`. A Reverb or a Delay bus needs `@m2/effects` in `music2.toml`, as the starter song has it.

## Notes

```json
{"clips": [
  {"id": "k3f9q2zt", "start": 0, "length": 4, "notes": [
    {"pitch": "F#2", "start": 0.5, "length": 0.25, "velocity": 0.9},
    {"pitch": 42, "start": 1.5, "length": 0.25}
  ]}
]}
```

- `pitch` is a note name (`"F#2"`, `"Bb3"`) or a number (60 is middle C, so `42` is F#2).
- A clip's `start` and `length` are in beats from the start of the song. A note's `start` and `length` are in beats from the start of its clip.
- `velocity` goes from 0 to 1. Without it, a note gets 0.8.
- `id` names the clip: 1 to 16 lowercase letters and digits, different from the other clips of the lane. music2 gives every clip it writes one; a clip without one still plays.
- music2 writes these files in one form, one clip and one note per line, so they diff well. The order of clips and notes in a file never changes what plays.

Automation and mask clips hold `points` instead of notes: `[beat, value]` pairs, with straight lines in between, and their file records the lane's unit (`{"unit": "hz", "clips": [...]}`). A lane that feeds a choice holds names instead, and one that feeds a switch `true` and `false`; each point holds until the next (`{"unit": "choice", "clips": [{"start": 0, "length": 8, "points": [[0, "saw"], [4, "square"]]}]}`). A name the choice doesn't have plays the lane's fallback there, with a warning. Audio clips name a file in `assets/`, pin it with `hash` (the first 16 hex digits of its SHA-256), and can have `offset`, `gain_db`, `fade_in`, `fade_out`, `reverse`, `pitch` and `warp`, as `data/chops/clips.json` does in the `resample` example ([Try it on a copy](changes.md#try-it-on-a-copy)).

## Units

A number in the code can carry a unit, written as a function. Plugging a value with the wrong unit into an input is an error. A plain number fits any unit.

| Write | Means |
|---|---|
| `hz(440)`, `khz(2.5)` | a frequency |
| `db(-6)` | a level in decibels |
| `linear(0.5)` | a gain as a plain multiplier |
| `semis(12)` | semitones (12 is an octave) |
| `secs(0.3)`, `ms(30)` | a time |
| `beats(0.5)` | a length in beats |
| `frac(0.5)` | a fraction from 0 to 1 |
| `bipolar(-0.3)` | a value from -1 to 1, like pan |
| `scalar(2)` | a number with no unit |
| `bpm(120)` | a tempo in beats per minute |

A choice and a switch aren't numbers. A choice is written by one of its names (`shape: "square"`), and a switch as `true` or `false`. Neither fits a number's input, and a choice fits only a choice with the same names in the same order.

## The SDK (`@m2/core`)

| Name | What it is |
|---|---|
| `track(key, { name }?, fn)` | a track: `fn` gets `{ key, lane, time }` and returns its sound, or an object of named outputs such as `{ out, env }`. `name` is its label; code never sees it |
| `song(fn)` | the root module in `song.ts`, with the key `song`: whatever it returns is what you hear |
| `lane.notes(key)`, `lane.control(key, unit, fallback)`, `lane.mask(key)`, `lane.audio(key)` | the timeline lanes, each with an optional last argument `{ name, shared }`. For a choice the unit is `"choice"` and the fallback a name (`lane.control("shape", "choice", "saw")`); for a switch, `"switch"` and `true` or `false` |
| `key(k, fn)` | runs `fn` in a frame named `k`, which files the saved settings of the plugins built inside it under `k` |
| `time.beats`, `time.seconds`, `time.playing` | the song's position, and 1 while it plays (0 while stopped). A track gets `time` in its `fn`; a component's body imports it from `@m2/core` |
| `time.tempo` | the song's tempo where it is, in beats per minute (`bpm`): for a sound that keeps time while the song is stopped. Only an input in `bpm` takes it |
| `time.key` | the song's key, as a set of notes: for a kernel that follows the key, as a tuner does. It isn't a number, so only a kernel's `control<keyset>` input takes it: bit 0 is C, bit 1 C♯, … bit 11 B, and with no key all twelve are on (4095). Changing the key while the song plays reaches it at once, and rebuilds nothing |
| `poly(notes, voice, { voices, mode, retrigger, steal, glide })` | plays a voice for each note at once (`voices`, 16); or one note at a time, each starting again (`mode: "mono"`) or sliding from the one held (`"legato"`) over `glide` seconds; `retrigger: false` carries a voice on into its next note; `steal` says which note stops when too many are held: `"oldest"`, `"quietest"` or `"none"`. Inside the voice, its note gives `pitch`, `gate`, `velocity`, `modWheel` and `keyCount` (how many notes its key has played on the lane, this one included: what `turns: "key"` counts) |
| `feedback(kind)` | a loop with a short delay; give its input later with `.set()` |
| `kernel(id, inputs)` | one kernel, such as `kernel("std:svf", { … })` |
| `plugin("clap:<id>", inputs)` | a CLAP plugin; `key` in its inputs names its saved settings (see [CLAP plugins](plugins.md)) |
| `component({ id, inputs, controls, settings, output, positional, editor }, body)` | declares a component: the signals it reads, its controls (knobs, each with a unit, a default and a range; or a choice, `control.choice(["saw", "square"], "saw")`, or a switch, `control.bool(false)`), its settings, and how the editor offers it, with its views and groups, and a label for any value whose name doesn't read as words, `setting.int(-1).label("sub octave")` ([Making a pack](making-a-pack.md#choices-switches-views-and-groups)). Every std function is one (std's code comes with music2: [where](changes.md#try-it-on-a-copy)); the older `component({ controls }, fn)`, which only marks which arguments are knobs, still works |

## Your own components

A function of your own becomes a box with knobs when it's declared as a component, as every std function is:

```ts
// lib/crunch.ts
import { component, control, kernel, port, signature, type Audio, type Params, type Value } from "@m2/core";

/** Drive, then a lowpass that tames the fizz. */
export const crunch = component(
  {
    id: "crunch",
    label: "Crunch",
    signature: signature<(x: Audio, amount?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      amount: control.db(12).range(0, 36).smooth(),
      tone: control.hz(3000).range(200, 20000).log().smooth(),
    },
    output: port.audio(),
    positional: ["amount"],
  },
  (x: Audio, { amount, tone }: Params) => kernel("std:svf", { in: kernel("std:drive", { in: x, amount }), cutoff: tone }).port("lp"),
);
```

- `id` is what the app knows the component by, whatever name a file imports it as. In a package it is `<package>:<id>` (`std:eq3`, `@m2/effects:delay`).
- `inputs` are the signals it reads. `port.audio({ sidechain: true })` marks one that only listens, like a compressor's key input.
- Each control has a unit and a default (`control.db(12)` is 12 dB), written in that unit, like every number in the descriptor. `.range(lo, hi)` is the knob's range (a hint: a number outside it is never changed), `.log()` turns it on a log scale, `.smooth()` glides a change over one block (for a control the sound reads every sample, so a jump would click), and `.optional()` means a call that leaves it out gets no node at all. The body always gets a signal for a control, whether the call wrote a number, connected a signal or left it out.
- `settings` are values that shape the graph, such as `setting.int(4)` voices: never knobs.
- `positional` lets calls write those controls as arguments, in order: `crunch(x, 18)`. Any control can also go in an options object after them: `crunch(x, { tone: 1500 })`. A misspelt option is an error that lists the right ones.
- `editor: { category, description, insert }` puts a package's component in the effect browser, under its category, and `insert` is what adding it writes (`{ amount: 6 }`).

## When something's wrong

`m2 check` reports a problem as the file, line and column, then what's wrong. A problem in the sound graph also names the module's key and the node in square brackets.

```
error: song.ts:14:14: snare is not defined [code.build]
error: tracks/hats.ts:5:62: [hats/n24] input `cutoff` expects control<hz>, got control<db> [code.check]
```

The first is a `snare: snare.out` channel in the mix of a song.ts that imports no `snare`. The second came from `lowpass(drums.drum(…), db(-6))` in `tracks/hats.ts`: a filter's cutoff wants a frequency, not a level. Both point at your own line. The word at the end says which step found it: `code.build` runs the code, and stops at its first problem; `code.check` checks what it built.

The app shows the same problems in the status pill's popover (click **Code doesn't compile** in the top bar) and in the code editor's list, with the file, the line and what's wrong, but without the part in square brackets.

Problems that don't stop the song (data no lane reads, a lane file that doesn't read, a missing asset or plugin) are printed by every `m2` command that opens the song, each with its code in square brackets, such as `[data.unclaimed]`, and the commands that fix it.

## Editing from the app

The code stays the source of truth; the app edits it for you, one small checked change at a time.

- A knob appears for a number written directly at an input: `0.3`, `hz(800)`, `db(-6)`, or a plain property like `{ pan: 0.3 }`.
- A knob also appears for every control a call leaves out, at the default its component declares (its number in grey). Its first turn writes the number into the call: as the next argument when that is where it goes (`lowpass(x, 1000)` becomes `lowpass(x, 1000, 0.8)`), else in the call's options (`{ release: ms(150), attack: 0.01 }`), else in new ones. A knob's range is the one its component declares.
- Computed values (`2 * 440`, a variable holding a plain number) and objects with spreads, getters or computed keys still work, but show **edit in code** instead of a knob.
- A shared value such as `const cutoff = hz(800)` shows as one box wired to every place it's used. Turning it changes all of them.
- Parentheses and comments around a number are kept.
- Before writing anything, the app compiles the new code and checks that only the intended thing changed: a knob changes only its number, a cable only what reads it, a rename nothing that plays.
- While the code doesn't compile, the last working version keeps playing and the knobs are locked.
- Cables inside one file, and adding or removing effects, also work. Cables between tracks, and adding or deleting other boxes, don't yet.
- Renaming a track writes, changes or takes out the `name` in its `track()` call (a name equal to the key takes it out, since the label defaults to the key). A name written as a variable is left alone: the app says to change it there.
- Automating a knob writes `lane.control("<key>", "<unit>", <the number as written>)` where the number was, adds `lane` to the track function's parameters when it isn't there (`() =>` becomes `({ lane }) =>`), and makes the empty lane file `data/<track>/<key>.json`. The key comes from the control's name (`decay`, or `hats-pan` for a mixer channel's), with the call's name in front only when the track has that key already (`pad-decay`); `{ name: … }` is written only when the key, read as words, isn't the control's label.
