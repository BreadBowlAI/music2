# Sounds and effects

**In short**
- music2's sounds come as packs that ship with it: `@m2/synths` (synths and FM sounds), `@m2/wavetable` (a wavetable synth), `@m2/drums` (a drum machine), `@m2/effects` (delay, reverb, chorus and five more) and `@m2/mixing` (an EQ, a gate, a transient shaper, a de-esser and a multiband compressor).
- std is the toolbox they are built from: oscillators, filters, envelopes, the Sampler, the mixer, and the four effects every mix starts with (`eq3`, `compress`, `limit`, `duck`). With the packs' effects that is 22 effects in seven groups.
- The packs install into your packs folder when music2 starts, and a song uses them as it uses any pack: listed in `music2.toml`, locked in `music2.lock`.

| Word | Meaning |
|---|---|
| **std** | music2's toolbox, in `std/`: what sounds are made of. TypeScript in `std/src`, kernels (the code that makes the sound) in `std/kernels/std.wasm`, built from Rust in `std/kernels-src` |
| **`@m2/` packs** | music2's own sounds, in `packs/@m2/`: ordinary [packs](packs.md) that call std's kernels, and their own (the synth, FM and drum voices, six of `@m2/effects`' effects, `@m2/mixing`'s tools and `@m2/wavetable`'s whole synth, built from Rust beside them). They come with the app and with `m2` |

A song that plays them lists them, as the templates do:

```toml
[dependencies]
"@m2/core" = "2"
std = "0.9"
"@m2/synths" = "2"
"@m2/effects" = "1"
```

```ts
import { synth } from "@m2/synths";
import { drums } from "@m2/drums";
import { reverb } from "@m2/effects";
```

When you add one of their sounds from + Track or the browser, music2 writes those lines for you, and `music2.lock` too.

A song made before std 0.8 is told, in words, what to change: first its std line, to `std = "0.9"`; then, for each import of `std/synth`, `std/fm`, `std/drums` or `std/instruments`, or of `std/fx`'s `reverb` and the other effects that colour a sound, which pack it is in now and the line to add. Code that calls the kernel `std:bitcrush` itself is told it is `@m2/effects:bitcrush` since std 0.8, and a song locked to `@m2/effects` 1.0.0 that plays `bitcrush` moves with `m2 pack update <song> @m2/effects`. The moves changed no sound: the song plays the same bits as before.

**Instruments.** Each takes a notes lane and returns sound. The `voices` setting is how many notes can sound at once (`synth.lead(notes, { voices: 4 })` plays chords). A typical note, at velocity 0.8, peaks between -15 and -12 dBFS, so a four-note chord still has room, and the drums sit as on a drum machine: the kick near -6, snare and clap near -9, hats near -16. Notes played harder are louder, and most sounds get brighter too. On the synths and FM sounds, `velSens` ("vel sens") is how much: the level follows velocity^(2 × velSens), so a note at velocity 0 is silent, and at 0.5 a note at 0.3 is 10 dB below one at 1. The keys start widest (0.8), the 808 narrowest (0.3).

- **Synths** (`@m2/synths`: `import { synth } from "@m2/synths"`), all one polysynth. On every one but the 808, `shape` is a menu of what its oscillators play: `"saw"` (the sound as it was made, its saws and any pulse it has), or one wave alone, as loud: `"square"` (hollow), `"triangle"` (soft) or `"sine"` (round, with no buzz): `synth.lead(notes, { shape: "square" })`. Each card shows its knobs in groups (Sound, Filter, Loudness), and draws its envelope (attack, decay, sustain, release) and its filter's curve (cutoff and resonance): drag a corner, or the filter's bump, to change them.
  - `synth.supersaw`: seven detuned saws, bright and wide, for big chords and hooks. Its filter starts wide open, as on a JP-8000; turn `cutoff` down to tame it. Knobs: `shape`, `detune`, `spread` (how wide), `cutoff`, `resonance`, `attack`, `decay`, `sustain`, `release`, `velSens`.
  - `synth.lead`: a singing lead that plays one note at a time. A note that starts before the last one ends slides into it, and a vibrato fades in on long notes. Knobs: `shape`, `cutoff`, `resonance`, `glide`, `vibrato`, `drive`, the envelope's four, `velSens`.
  - `synth.pad`: a big, slow, wide pad that swells open. Knobs: `shape`, `cutoff`, `resonance`, the envelope's four, `spread`, `velSens`.
  - `synth.pluck`: a bright, wide pluck for arps and chords, whose filter snaps shut. Knobs: `shape`, `brightness`, `spread`, the envelope's four (its `decay` closes the filter too), `velSens`.
  - `synth.sub`: an 808 bass, one note at a time: a deep, warm sine that drops into each note (by `drop` semitones), rings out over `decay` (to -60 dB: 0.2 s for a tight hit, 4 s by default, longer for long slides) and slides between overlapping notes. Knobs: `drop`, `drive`, `glide`, the envelope's four, `velSens`.
  - `synth.bass`: a synth bass, one note at a time: a saw and a pulse with a sine under them, through a filter that bites as each note starts. Knobs: `shape`, `cutoff`, `resonance`, `bite` (how many octaves the filter opens), `biteDecay` (how fast it closes), `glide`, the envelope's four, `velSens`.
  - `synth.strings`: a string machine, soft saws through an ensemble chorus. Knobs: `shape`, `ensemble`, `cutoff`, `resonance`, the envelope's four, `velSens`.
  - `synth.custom`: the synth itself with every knob, to make your own sound: its `shape`; the saw's and the pulse's levels (`saw` and `square`) and the pulse's `width`, which at another shape set only how loud that shape plays; sub and noise; a ladder filter with its own envelope, the amp envelope, drive, vibrato, a pitch drop and glide. `unison` (1 to 7 stacked voices) is a setting: `synth.custom(notes, { unison: 5 })`.
  - With `voices: 1` any of them, and any FM sound, plays one note at a time: a note that starts before the last one ends carries on from it, without starting again (legato), and the ones with a `glide` knob slide to it. When that note ends while an older one is still held, the older one sounds again, sliding back: hold A1 on the 808 and tap A2 over it for the classic slide up and back. Notes that start together count as one, the last of them, and each note keeps its own velocity. `synth.lead`, `synth.sub` and `synth.bass` start that way, and their card in Device says "Plays one note at a time". For a voice of your own, std's `mono(notes, voice, { glide })`, in `std/dsp`, is `poly(notes, voice, { mode: "legato", retrigger: false, glide })`.
- **FM** (`@m2/synths`: `import { fm } from "@m2/synths"`), one FM synth: `fm.keys`, an electric piano with a warm body and a tine that barks when you play hard (knobs `tone`, `tine`, the envelope's four, `velSens`); `fm.bell`, a glassy bell (`tone`, the envelope's four, `velSens`); `fm.metal`, a short, clangy metallic hit that zaps down into its note (`tone`, `decay`, `feedback`, `drop`, `velSens`); and `fm.custom`, the FM synth with every knob. Its carriers and modulators are frequency ratios to the note: whole numbers sound harmonic, halves (the bell's 3.5) like a bell, and others such as 3.17 clang like metal. A ratio typed into the code is exact; dragged on a knob it lands near a value (2.03, say), so type the musical ones.
- **Wavetable** (`@m2/wavetable`: `import { wavetable } from "@m2/wavetable"`), the kind of synth most producers use today. A wavetable is a set of waveforms, its frames; `position` slides between them, and as it moves the sound changes shape. Turn it slowly, or let the LFO turn it, and the sound moves. Two oscillators, A and B, each play a table, chosen from its menu on the card (`tableA`, `tableB`):

  | Table | Its frames, from position 0 to 1 |
  |---|---|
  | `basic` | a sine (soft), a triangle, a saw (bright), a square (hollow) |
  | `pulse` | a square that gets narrower, to a thin, nasal pulse |
  | `vowels` | a voice singing a, e, i, o, u |
  | `harmonics` | a sine, then harmonics added one by one, up to a bright saw |
  | `digital` | hard, glassy waves, harmonics switched on and off in steps |

  Or play your own: drop a WAV on the oscillator's file on the card (`fileA`, `fileB`), and it is that oscillator's table, in place of the menu's. A wavetable file is frames of 2048 samples one after another, as Serum and Vital save theirs; a file shorter than that is one cycle of a wave (a single-cycle file). Up to 64 frames are kept: a file with more gives 64 spread evenly through it. Each oscillator has `position`, `pitch` (in semitones: 12 is an octave up) and `level`; B starts silent (`levelB` 0). Then a filter: `filter` is its shape (`lowpass` keeps the lows, `highpass` the highs, `bandpass` the middle, `notch` all but the middle), with `cutoff`, `q` (the bump at the cutoff), `keytrack` (higher notes open it more) and `filterEnv` (how many octaves its own envelope opens it). Two envelopes, each with its picture on the card: the amp's (`attack`, `decay`, `sustain`, `release`) and the filter's (`filterAttack` and the rest). An LFO, a slow wave from each note's start: `lfoRate`, how far it moves both positions (`lfoPosition`) and the cutoff (`lfoCutoff`, in octaves). `unison` (a setting, 1 to 8) plays each note as that many copies, `detune` semitones apart at the edges and `spread` wide. With `voices: 1` it plays one note at a time, sliding over `glide`. Its presets: Slow pad, Round bass, Sharp lead and Short pluck; a preset sets the knobs, not the tables. A high note never folds back into a low hum (aliasing): each table is kept as a copy for each octave, holding only the harmonics that fit under it.
- **Drums** (`@m2/drums`: `import { drums } from "@m2/drums"`), an 808/909-style drum machine. `drums.kit` plays a whole beat from one notes lane, each drum on its General MIDI note, the numbers drum parts usually use:

  | Note | Drum | Note | Drum |
  |---|---|---|---|
  | C2 (36), B1 (35) | kick (B1 lower and softer) | F#2 (42) | closed hat |
  | D2 (38), E2 (40) | snare (E2 higher and snappier) | G#2 (44) | pedal hat |
  | D#2 (39) | clap | A#2 (46) | open hat |
  | C#2 (37) | rim | C#3 (49), A3 (57) | crash |
  | F2, G2, A2, B2, C3, D3 (41 to 50) | toms, low to high | D#3 (51), B3 (59) | ride |
  | G#3 (56) | cowbell | F#3 (54) | tambourine |
  | A4, A#4 (69, 70) | shakers | D#5 (75) | claves |

  A hit cuts off the drum before it, as on a drum machine: a kick the kick still ringing, a closed hat an open one. Toms, rim, cowbell, ride and shakers ring on. No two hits are quite the same, as on an analog machine (each catches its noise and metal at a new place), yet every render of a song is. The kit's knobs are `kickTune`, `kickDecay`, `punch` (from a deep 808 boom to a hard 909 thump), `snareTune`, `snap`, `hatDecay`, `openDecay`, `hatTone`, `tomTune`, `drive`, and a level in dB for each group: `kickLevel`, `snareLevel` (with the clap and rim), `hatLevel`, `tomLevel` and `percLevel` (cymbals and the rest). Its card shows them in groups: Kick, Snare & clap, Hats, Toms and Percussion.

  `drums.drum` (**One drum** in **+ Track**) plays one drum on every note, the one its menu picks: `drum: "kick"` (the default), `"snare"`, `"clap"`, `"hats"` (closed, with G#2 pedal and A#2 open), `"tom"` (tuned by the note: A2 plays at the drum's own pitch), `"rim"`, `"cowbell"`, `"cymbal"` or `"shaker"`. Its menu is "sound" on its card: a pick plays from the next hit, and a lane can change it as the song plays (Automate). Its knobs move whichever drum it plays: `tune` (semitones up or down from the drum's own), `decay` ("decay ×" on the card: how long it rings, times its own; 2 is twice as long), `punch` ("punch ±": more, or less, of a kick's thump or a snare's snap, and on the others of each hit's first moment; from -1 to 1) and `drive`. A kick, snare, clap, hats or cymbal cuts off the one before it at its next hit; a tom, rim, cowbell or shaker rings on. The kit comes out as one sound, so a drum you want to treat on its own (a kick for the bass to duck under, a snare with a reverb of its own) goes on a lane of its own, played by One drum, as `examples/sounds` does with its kick: `drums.drum(lane.notes("kick"), { tune: 0.7 })`.
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
  - Whose turn it is: `turns` says how the takes count.

    | `turns` | Counts | A kick and a snare that alternate on one lane |
    |---|---|---|
    | `"lane"` (the default) | every note the lane plays, whatever its key; notes that start together count once | each drum plays the same take every time |
    | `"key"` (std 0.6.0) | each key its own notes, as SFZ instruments do; a chord's notes each count on their own key | each drum takes 1, 2, 1, 2 |

    For drums or a piano made of recordings, `turns: "key"` is usually what you want: `sampler(notes, { regions, turns: "key" })`.
  - The turns are the same in an export, and when you open the song and play it from the start. Play it again (Return, then Space), jump, or try notes on the lane, and the turns carry on from where they were, so a take may swap.
  - Every take of a round robin must be there wherever it plays: `[1, 3]` and `[2, 3]` with no `[3, 3]` is refused, since every third note would play nothing.
  - A `"release"` region plays to its end when the note stops, whatever `mode` says.
  - A loop repeats while the key is held. It blends its last few milliseconds into the start, so it doesn't click. When you let go, the file plays on past the loop to its end (in `mode: "gate"`, fading over `release`).
  - A mistake is said in words when the song builds: "regions: region 3: keys 70 to 60: the lowest is above the highest".
  - Regions can come from a helper function or a `const`, as a list of many files is easiest to write. A `trigger` kept in a `const` needs the const's type, or the code editor reads it as any text: `const keyUps: Region[] = [{ file: "key-up.wav", trigger: "release" }]`, with `import { type Region } from "@m2/core"`.

  The Sampler's card in Device says what it plays ("5 recordings, 2 layers, round robin of 2, release sounds, loops"). Its regions are changed in code: a WAV dropped on it is refused. A MIDI keyboard's sustain pedal holds its notes, as it holds every instrument's ([The window](window.md)). Not yet: choke groups (an open hat cut by a closed one), blends between velocity layers, and instruments too big for memory ([gap #22](https://github.com/BreadBowlAI/music2/blob/main/docs/gaps.md#sounds)).
- **The first instruments** (`@m2/synths`): `bass`, `pluck` and `pad`, simple versions built from std's building blocks below, short enough to read and copy. Code plays them; the lists leave them out. For a song, the ones above sound fuller: `synth.bass` or `synth.sub`, `synth.pluck` and `synth.pad`. The drums are the kit's (`drums.kit`) and One drum's (`drums.drum`); the starter song plays its kick and hats on One drum.

[`examples/sounds`](https://github.com/BreadBowlAI/music2/tree/main/examples/sounds) plays every synth and drum, and the FM keys, bell and metal, in a short pop song, mastered near -11.5 LUFS.

**Effects**, as the effect browser groups them. Eight are `@m2/effects`', five `@m2/mixing`'s; the filters, `eq3`, `compress`, `limit`, `gain` and `pan` are std's, since every mix needs them:

| Group | Effects |
|---|---|
| Filter | `lowpass`, `highpass`, `bandpass`, `notch` (std, `std/filter`) |
| EQ | `eq3` (std, `std/fx`), `eq` (`@m2/mixing`) |
| Dynamics | `compress`, `limit` (std, `std/fx`), `gate`, `transient`, `deesser`, `multiband` (`@m2/mixing`) |
| Distortion | `drive`, `bitcrush` (`@m2/effects`) |
| Delay & Reverb | `delay`, `reverb` (`@m2/effects`) |
| Modulation | `chorus`, `phaser`, `tremolo` (`@m2/effects`) |
| Utility | `gain`, `pan` (std, `std/dsp`), `widen` (`@m2/effects`) |

Chorus, phaser and tremolo move only while the song plays. Stopped, they wait, and Play carries them on from there. The bitcrusher keeps crushing a tail after Stop, and at Play it picks up where it stopped. So these effects, and the drums, sound as in the export when you press Play, however long the song sat first. Going back to bar 1 after playing is a jump: the wobble carries on from where it stopped, so that play may differ a little from the export. An oscillator or `noise` written straight into a track's code, outside a voice, runs on while the song is stopped, so it may start from another place than in the export. A plugin may run on too.

**`duck`** (`std/fx`) is the sidechain: `duck(bass.out, kick.env)` turns the bass down while the kick plays. It needs a second signal, so it's written in code rather than added from the browser.

**Mixing tools** (`@m2/mixing`: `import { eq, gate } from "@m2/mixing"`). Each is an effect: add it after any sound from + Effect, or write it in code.

| Tool | What it is for | Its knobs |
|---|---|---|
| `eq`, Parametric EQ | shaping the tone: taking out rumble or harshness, or bringing a part forward | eight bands, numbered 1 to 8, each with `shape` (a menu), `freq`, `gain`, `q` and `on` (a switch): `eq(x, { gain3: 4, freq3: 300 })` |
| `gate` | silencing a track between its sounds: the hum between drum hits, the breath between lines | `threshold`, `attack`, `hold`, `release`, `range` |
| `transient`, Transient shaper | making each hit punchier or softer, and its ring longer or shorter | `attack`, `sustain` |
| `deesser`, De-esser | taming the hiss of S and T sounds in a voice | `freq`, `threshold`, `amount` |
| `multiband`, Multiband compressor | compressing the lows, mids and highs each on its own, so a booming bass doesn't squeeze the voice | `lowMid`, `midHigh`, then for each band (`low`, `mid`, `high`) its threshold, ratio, attack, release and gain: `lowRatio`, `midGain` … |

| Word | Meaning |
|---|---|
| **band** | a part of the sound, from its low notes to its high ones |
| **shape** | what an EQ band does: a *low cut* takes out what is below its frequency, a *high cut* what is above; a *low shelf* or *high shelf* turns everything below or above up or down; a *bell* turns the notes around its frequency up or down |
| **Q** | how narrow a bell is (higher is narrower), how sharp a cut's corner, or how big a shelf's bump and dip at its corner (above 0.71; a lot of both at 18) |
| **threshold** | the level, in dB, where a tool starts to act |
| **attack**, **release** | how fast a tool acts, and how fast it lets go |
| **hold** | how long a gate stays open once the sound gets quiet |
| **range** | how far down a closed gate turns the sound: −80 dB is near silence, −10 dB only a little quieter |
| **ratio** | how hard a compressor squeezes: at 2, a level 10 dB over the threshold comes out 5 dB over; 1 is off |
| **sidechain** | a sound that only steers a tool, and isn't heard through it |
| **transient** | the very start of a hit: its crack, click or thump |
| **sustain** | the rest of a hit, as it rings and fades |

- **The EQ** starts flat: band 1 (a low cut at 30 Hz) and band 8 (a high cut at 18 kHz) are off, the six between at 0 dB, so adding it changes nothing until you turn a knob. Its card shows each band under its own heading, `Band 1` to `Band 8`. A cut takes out 12 dB more each octave further away; its `gain` does nothing. Every band is the standard filter of its shape (RBJ's, as in most EQs). A band moved while it plays crossfades to its new setting, so a change never clicks or booms; a big change on a very resonant band (a high Q, a low frequency) waits up to a second for the new filter to settle before you hear it. A small move, a knob's drag or a lane's line, is heard at once. The card draws no curve yet.
- **The gate** opens when the sound reaches `threshold`, and closes once it has stayed 6 dB under it for `hold` (at least 25 ms, so a low note's waves don't close it). Keyed by another track, `gate(pad.out, kick.out)` opens the pad only while the kick plays: the kick is its sidechain, so muting the kick doesn't change it. Its key is written in code for now.
- **The transient shaper** turns a hit's start by up to `attack` dB, and its tail by up to `sustain` dB. The faster a tail fades, the more of `sustain` it gets: a snare's short ring all of it, a piano's long fade a quarter or so in its first half second, more as it goes on. A steady sound passes as it is.
- **The de-esser** turns down the band around `freq` (5 to 8 kHz for most voices), a bell about an octave and a half wide, deepest at `freq`, and only while that band is louder than `threshold`, by at most `amount` dB. While it works, a loud tone an octave away loses a dB or so; a voice's body, two octaves and more below, is left alone.
- **The multiband compressor** splits the sound where `lowMid` and `midHigh` say, with the crossovers most multiband compressors use (Linkwitz-Riley's); if they cross, the lower one splits the lows from the mids. At ratio 1, with every band's `gain` at 0, it changes no level at any frequency; like every crossover of its kind, it turns the phase a little.
- Each has a few presets on its card's **Presets** (`Tight drums`, `Punchy drums`, `Bright voice`, `Gentle glue` …).

**`mixer` and `master`** (`std/mix`): `mixer` adds tracks together. Each channel has a level, `db` (0 when left out), and can have a `pan`, `sends` to buses and a `to` (a group bus instead of the master): see [Buses, sends and groups](https://github.com/BreadBowlAI/music2/blob/main/docs/song-as-program.md#buses-sends-and-groups). `master` has a level too, then a limiter, so the song never clips. A level of −96 dB or lower is silence.

**Building blocks** (std): `std/dsp` (oscillators, noise, envelopes, filters, gain, pan, and `mono` for a voice of your own), `std/math` (add, scale, blend, sum, pitch to frequency, ranges), `std/control` (`lfo`, `when`, `choose`) and `std/filter`. A pack's sound is built from these and from std's kernels: `@m2/synths`' code is a good place to read how.

`m2 kernels` lists every kernel with its typed inputs and outputs, and `m2 effects <song>` lists the effects (by their ids, such as `@m2/effects:delay` or `std:eq3`) and where they can go.
