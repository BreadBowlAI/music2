# Sounds and effects

**In short**
- std is the package of sounds that comes with music2: synths, FM sounds, a drum machine and a Sampler.
- It has 17 effects in seven groups, a sidechain (`duck`), and the mixer and master.

std is an ordinary package in `std/`: TypeScript in `std/src`, kernels in `std/kernels/std.wasm`, built from Rust in `std/kernels-src`.

**Instruments.** Each takes a notes lane and returns sound. The `voices` setting is how many notes can sound at once (`synth.lead(notes, { voices: 4 })` plays chords). A typical note, at velocity 0.8, peaks between -15 and -12 dBFS, so a four-note chord still has room, and the drums sit as on a drum machine: the kick near -6, snare and clap near -9, hats near -16. Notes played harder are louder, and most sounds get brighter too. On the synths and FM sounds, `velSens` ("vel sens") is how much: the level follows velocity^(2 × velSens), so a note at velocity 0 is silent, and at 0.5 a note at 0.3 is 10 dB below one at 1. The keys start widest (0.8), the 808 narrowest (0.3).

- **Synths** (`std/synth`), all one polysynth:
  - `synth.supersaw`: seven detuned saws, bright and wide, for big chords and hooks. Its filter starts wide open, as on a JP-8000; turn `cutoff` down to tame it. Knobs: `detune`, `spread` (how wide), `cutoff`, `attack`, `release`, `velSens`.
  - `synth.lead`: a singing lead that plays one note at a time. A note that starts before the last one ends slides into it, and a vibrato fades in on long notes. Knobs: `cutoff`, `glide`, `vibrato`, `drive`, `velSens`.
  - `synth.pad`: a big, slow, wide pad that swells open. Knobs: `cutoff`, `attack`, `release`, `spread`, `velSens`.
  - `synth.pluck`: a bright, wide pluck for arps and chords, whose filter snaps shut. Knobs: `brightness`, `decay`, `spread`, `velSens`.
  - `synth.sub`: an 808 bass, one note at a time: a deep, warm sine that drops into each note (by `drop` semitones), rings out over `decay` (to -60 dB: 0.2 s for a tight hit, 4 s by default, longer for long slides) and slides between overlapping notes. Knobs: `drop`, `drive`, `glide`, `decay`, `velSens`.
  - `synth.bass`: a synth bass, one note at a time: a saw and a pulse with a sine under them, through a filter that bites as each note starts. Knobs: `cutoff`, `bite` (how many octaves the filter opens), `decay` (how fast it closes), `glide`, `velSens`.
  - `synth.strings`: a string machine, soft saws through an ensemble chorus. Knobs: `cutoff`, `attack`, `release`, `ensemble`, `velSens`.
  - `synth.custom`: the synth itself with every knob, to make your own sound: saw, pulse, sub and noise, a ladder filter with its own envelope, the amp envelope, drive, vibrato, a pitch drop and glide. `unison` (1 to 7 stacked voices) is a setting: `synth.custom(notes, { unison: 5 })`.
  - With `voices: 1` any of them, and any FM sound, plays one note at a time: a note that starts before the last one ends carries on from it, without starting again (legato), and the ones with a `glide` knob slide to it. When that note ends while an older one is still held, the older one sounds again, sliding back: hold A1 on the 808 and tap A2 over it for the classic slide up and back. Notes that start together count as one, the last of them, and each note keeps its own velocity. `synth.lead`, `synth.sub` and `synth.bass` start that way, and their card in Device says "Plays one note at a time". For a voice of your own, `mono(notes, voice, { glide })` is `poly(notes, voice, { mode: "legato", retrigger: false, glide })` (std 0.3.0).
- **FM** (`std/fm`), one FM synth: `fm.keys`, an electric piano with a warm body and a tine that barks when you play hard (knobs `tone`, `tine`, `decay`, `release`, `velSens`); `fm.bell`, a glassy bell (`tone`, `decay`, `velSens`); `fm.metal`, a short, clangy metallic hit that zaps down into its note (`tone`, `decay`, `feedback`, `drop`, `velSens`); and `fm.custom`, the FM synth with every knob. Its carriers and modulators are frequency ratios to the note: whole numbers sound harmonic, halves (the bell's 3.5) like a bell, and others such as 3.17 clang like metal. A ratio typed into the code is exact; dragged on a knob it lands near a value (2.03, say), so type the musical ones.
- **Drums** (`std/drums`), an 808/909-style drum machine. `drums.kit` plays a whole beat from one notes lane, each drum on its General MIDI note, the numbers drum parts usually use:

  | Note | Drum | Note | Drum |
  |---|---|---|---|
  | C2 (36), B1 (35) | kick (B1 lower and softer) | F#2 (42) | closed hat |
  | D2 (38), E2 (40) | snare (E2 higher and snappier) | G#2 (44) | pedal hat |
  | D#2 (39) | clap | A#2 (46) | open hat |
  | C#2 (37) | rim | C#3 (49), A3 (57) | crash |
  | F2, G2, A2, B2, C3, D3 (41 to 50) | toms, low to high | D#3 (51), B3 (59) | ride |
  | G#3 (56) | cowbell | F#3 (54) | tambourine |
  | A4, A#4 (69, 70) | shakers | D#5 (75) | claves |

  A hit cuts off the drum before it, as on a drum machine: a kick the kick still ringing, a closed hat an open one. Toms, rim, cowbell, ride and shakers ring on. No two hits are quite the same, as on an analog machine (each catches its noise and metal at a new place), yet every render of a song is. The kit's knobs are `kickTune`, `kickDecay`, `punch` (from a deep 808 boom to a hard 909 thump), `snareTune`, `snap`, `hatDecay`, `openDecay`, `hatTone`, `tomTune`, `drive`, and a level in dB for each group: `kickLevel`, `snareLevel` (with the clap and rim), `hatLevel`, `tomLevel` and `percLevel` (cymbals and the rest). Each drum is also an instrument of its own, on every note: `drums.kick`, `drums.snare`, `drums.clap`, `drums.hats` (closed, with G#2 pedal and A#2 open), `drums.tom` (tuned by the note: A2 plays at its `tune`), `drums.rim`, `drums.cowbell`, `drums.cymbal` and `drums.shaker`. The kit comes out as one sound, so a drum you want to treat on its own (a kick for the bass to duck under, a snare with a reverb of its own) goes on a lane of its own, played by that drum alone, as `examples/sounds` does with its kick.
- **Sampler** (`std/sampler`, std 0.2.1): `sampler` plays a WAV file from the song's `assets/` folder on every note, higher or lower by how far the note is from `root` (60, C4): `sampler(lane.notes("notes"), { sample: "pluck.wav" })`. Or one file for each range of notes, each from its root up to the next one's: `{ zones: { 36: "kick.wav", 38: "snare.wav" } }`. It plays the whole file however short the note (`mode: "oneshot"`), or stops with the note, fading over `release` (`mode: "gate"`). Knobs: `tune` (semitones), `level`, `attack`, `release`, and `velocity` (std 0.5.0): how much playing harder makes a note louder, from 0 (not at all) to 1 (all the way, the default). In the app, drop a WAV on its card in Device. [`examples/sampler`](https://github.com/BreadBowlAI/music2/tree/main/examples/sampler) plays drums and a melody from three short files.

  **Recorded instruments: `regions`** (std 0.5.0). A real piano, guitar or drum kit is many recordings: each note, played soft and hard, sometimes twice so repeats don't sound the same. `regions` maps them, one region a recording, with the ideas of SFZ (the open format free sampled instruments use). Give one of `sample`, `zones` and `regions`. Only `file` is needed; the rest have defaults:

  | Field | What it means | Left out |
  |---|---|---|
  | `file` | the recording, in `assets/` | needed |
  | `root` | the note it was recorded at (60 is C4) | 60 |
  | `keys` | `[lowest, highest]`: the notes it plays, higher or lower than its root | its root alone if you give `root`, else every note |
  | `velocity` | `[lowest, highest]`: how hard, 1 (softest) to 127 (hardest) | every velocity |
  | `round_robin` | `[position, of]`: one of several takes, played in turn: `[1, 2]` and `[2, 2]` alternate | `[1, 1]` |
  | `trigger` | `"attack"`: plays when the note starts; `"release"`: when it ends (a key's click, a string's buzz) | `"attack"` |
  | `loop` | `[start, end]`: frames of the file (samples of each channel, counted from 0) to repeat while the note is held | no loop |
  | `tune` | cents (a hundredth of a semitone) | 0 |
  | `volume` | dB | 0 |
  | `pan` | -1 (left) to 1 (right) | 0 |
  | `offset` | frames into the file to start from | 0 |

  ```ts
  sampler(lane.notes("notes"), {
    regions: [
      { file: "piano/C4-soft-1.wav", keys: [58, 62], velocity: [1, 80], round_robin: [1, 2] },
      { file: "piano/C4-soft-2.wav", keys: [58, 62], velocity: [1, 80], round_robin: [2, 2] },
      { file: "piano/C4-hard.wav", keys: [58, 62], velocity: [81, 127] },
      { file: "piano/key-up.wav", trigger: "release", volume: -12 },
    ],
    mode: "gate",
  });
  ```

  How it plays:
  - A note's velocity, 0 to 1 in the piano roll, is a number from 1 to 127 here: velocity × 127, rounded. So 0.5 is 64.
  - Every region whose `keys`, `velocity` and turn match plays, all at once (layers). A pad region under every note, say, plays with the piano's.
  - The turn counts the notes the lane has played, whatever their key: notes that start together count once, so a chord's notes all take the same turn. So notes that take turns on one lane keep their takes: a kick and a snare that alternate (kick, snare, kick, snare) each play the same take every time, and so do the two notes of a trill. Give each drum whose takes must alternate a lane of its own. (A turn for each key is [gap #22](https://github.com/BreadBowlAI/music2/blob/main/docs/gaps.md#sounds).)
  - The turns are the same in an export, and when you open the song and play it from the start. Play it again (Return, then Space), jump, or try notes on the lane, and the turns carry on from where they were, so a take may swap.
  - Every take of a round robin must be there wherever it plays: `[1, 3]` and `[2, 3]` with no `[3, 3]` is refused, since every third note would play nothing.
  - A `"release"` region plays to its end when the note stops, whatever `mode` says.
  - A loop repeats while the key is held. It blends its last few milliseconds into the start, so it doesn't click. When you let go, the file plays on past the loop to its end (in `mode: "gate"`, fading over `release`).
  - A mistake is said in words when the song builds: "regions: region 3: keys 70 to 60: the lowest is above the highest".
  - Regions can come from a helper function or a `const`, as a list of many files is easiest to write. A `trigger` kept in a `const` needs the const's type, or the code editor reads it as any text: `const keyUps: Region[] = [{ file: "key-up.wav", trigger: "release" }]`, with `import { type Region } from "@m2/core"`.

  The Sampler's card in Device says what it plays ("5 recordings, 2 layers, round robin of 2, release sounds, loops"). Its regions are changed in code: a WAV dropped on it is refused. Not yet: a sustain pedal, choke groups (an open hat cut by a closed one), blends between velocity layers, and instruments too big for memory ([gap #22](https://github.com/BreadBowlAI/music2/blob/main/docs/gaps.md#sounds)).
- **The first instruments** (`std/instruments`): `bass`, `kick`, `hat`, `pluck` and `pad`, simple versions built from the building blocks below, short enough to read and copy. For a song, the ones above sound fuller: `synth.bass` or `synth.sub`, `drums.kick`, `drums.hats`, `synth.pluck` and `synth.pad`. The templates still start new songs with the first ones.

[`examples/sounds`](https://github.com/BreadBowlAI/music2/tree/main/examples/sounds) plays every synth and drum, and the FM keys, bell and metal, in a short pop song, mastered near -11.5 LUFS.

**Effects**, as the effect browser groups them:

| Group | Effects |
|---|---|
| Filter | `lowpass`, `highpass`, `bandpass`, `notch` |
| EQ | `eq3` |
| Dynamics | `compress`, `limit` |
| Distortion | `drive`, `bitcrush` |
| Delay & Reverb | `delay`, `reverb` |
| Modulation | `chorus`, `phaser`, `tremolo` |
| Utility | `gain`, `pan`, `widen` |

Chorus, phaser and tremolo move only while the song plays. Stopped, they wait, and Play carries them on from there. The bitcrusher keeps crushing a tail after Stop, and at Play it picks up where it stopped. So these effects, and std's drums, sound as in the export when you press Play, however long the song sat first. Going back to bar 1 after playing is a jump: the wobble carries on from where it stopped, so that play may differ a little from the export. An oscillator or `noise` written straight into a track's code, outside a voice, runs on while the song is stopped, so it may start from another place than in the export. A plugin may run on too.

**`duck`** (`std/fx`) is the sidechain: `duck(bass.out, kick.env)` turns the bass down while the kick plays. It needs a second signal, so it's written in code rather than added from the browser.

**`mixer` and `master`** (`std/mix`): `mixer` adds tracks together. Each channel has a level, `db` (0 when left out), and can have a `pan`, `sends` to buses and a `to` (a group bus instead of the master): see [Buses, sends and groups](https://github.com/BreadBowlAI/music2/blob/main/docs/song-as-program.md#buses-sends-and-groups). `master` has a level too, then a limiter, so the song never clips. A level of −96 dB or lower is silence.

**Building blocks:** `std/dsp` (oscillators, noise, envelopes, filters, gain, pan), `std/math` (add, scale, blend, sum, pitch to frequency, ranges), `std/control` (`lfo`, `when`, `choose`) and `std/filter`.

`m2 kernels` lists every kernel with its typed inputs and outputs, and `m2 effects <song>` lists the effects (by their ids, such as `std:delay`) and where they can go.
