# Music basics

The music ideas music2 uses, in plain words, for someone who has never made a song. Each part ends with where the idea lives in music2. Words about the app itself (track, lane, clip, knob…) are in the README's [Words you'll see](../README.md#words-youll-see).

## Sound, notes and pitch

- A **sound** is air vibrating. How fast it vibrates is its **pitch**: faster sounds higher.
- A **note** is one sound at one pitch. It starts at one moment and lasts for a while.
- Notes are named with a letter and an octave number. `C4` is "middle C" on a piano, `A4` is the note orchestras tune to, and `C5` is the C one octave higher (it vibrates twice as fast).
- There are 12 notes in an octave: `C C# D D# E F F# G G# A A# B`. On a piano, these are all the white and black keys.
- **Velocity** is how hard a note is played. Harder is louder, and usually brighter.

**In music2:** notes are drawn in the Notes view (the piano roll). A pitch can be written as a name (`"C4"`) or a number (60 is middle C). Velocity goes from 0 to 1, and a new note gets 0.8; the strip at the bottom of the Notes view sets it.

## Notes or recordings

This is the most important difference in music software.

| | **Notes** (often called MIDI) | **Audio** (a recording) |
|---|---|---|
| What it is | Instructions: "play C4 at beat 1, for half a beat, fairly hard" | The sound wave itself, like a WAV or MP3 file |
| Like | Sheet music | A recording of someone playing |
| Can you change it? | Completely: any note, or the whole instrument | Mostly as a whole: cut, move, stretch |
| Makes sound by | Being played by an **instrument** | Just playing back |

**In music2:** a notes lane (`lane.notes`) holds clips of notes, and an audio lane (`lane.audio`) holds clips that play part of a WAV file from the song's `assets/` folder. Audio clips can't be edited in the app yet ([gap #18](gaps.md#audio)).

## Time: tempo, beats, bars and the grid

- **Tempo** is the speed of the song in **BPM** (beats per minute). Dance-pop is usually 118 to 128.
- A **beat** is one pulse: the thing you tap your foot to.
- A **bar** (or measure) is a group of beats. Almost all pop uses **4/4 time**: 4 beats per bar. Song parts are counted in bars ("an 8-bar chorus").
- The **grid** splits beats into smaller steps for placing notes. **1/16** means four steps per beat; that's where hi-hats and fast patterns live. **Quantize** snaps notes onto the grid, and **humanize** nudges them a little so they sound less robotic.

**In music2:** the tempo and the time signature are in `data/arrangement.json`. The tempo shows at the top left of the timeline, and the ruler counts bars by the time signature, but changing either means editing that file for now ([gap #6](gaps.md#song-settings)). The Notes view snaps to 1/4, 1/8, 1/16 or 1/32, but not yet to triplets (three notes in the space of two). Quantize and humanize aren't in the app yet ([gap #15](gaps.md#notes)).

## Key, scale and chords

- A **scale** is a set of 7 of the 12 notes that sound good together. **Major** sounds bright and happy; **minor** sounds emotional and moody. A lot of dance-pop is in minor, which is how it sounds happy and sad at once.
- The **key** is a scale plus its home note, such as "A minor". A minor uses only the white keys: A B C D E F G.
- A **chord** is several notes played together, usually 3 (a **triad**).
- A **chord progression** is the order of chords, usually one per bar, repeating. Chords are named by their place in the scale with Roman numerals. The most famous pop progression is **I–V–vi–IV** (in C major: C, G, Am, F). In minor keys, **i–VI–III–VII** is a classic.

**In music2:** there's no key setting yet, and no way to hide the notes outside the key in the piano roll. Scale help, chord stamps and an arpeggiator are planned ([gap #16](gaps.md#notes)). For now, pick the notes of your scale by hand.

## The parts of a pop song

| Part | What it is | Typical sound |
|---|---|---|
| **Drums** | The rhythm: the **kick** (the boom), the **snare** or **clap** (the crack on beats 2 and 4), the **hi-hats** (the tick-tick in between) | "Four on the floor": a kick on every beat, the heartbeat of disco and house |
| **Bass** | Low notes that lock in with the kick and outline the chords | Often a pumping note between kicks |
| **Chords or pad** | The harmony, a cushion of sound under everything | Lush and held, or short rhythmic **stabs** |
| **Melody, hook or topline** | The tune you sing along to. The **hook** is the catchiest bit | A bright synth lead, or a voice |
| **Counter-melody** | A second tune that answers the melody in its gaps | Plucks, strings |
| **Arp** (arpeggio) | A chord played one note at a time, fast and repeating | Sparkly 16th-note patterns |

**In music2:** the drums are `drums.kit` (one lane plays kick, snare, clap, hats, toms and more), or each drum alone, such as `drums.snare` ([`std/src/drums.ts`](../std/src/drums.ts)). For the rest: `synth.sub` (an 808 bass) and `synth.bass` (a synth bass), `synth.supersaw` and `synth.pad` (chords), `synth.lead` (a hook that glides), `synth.pluck` (arps), `synth.strings`, `fm.keys` and `fm.bell`, and `fm.metal` (metallic hits) ([`std/src/synth.ts`](../std/src/synth.ts), [`std/src/fm.ts`](../std/src/fm.ts)). The first, simpler instruments are in [`std/src/instruments.ts`](../std/src/instruments.ts). [`examples/sounds`](../examples/sounds) uses them all.

## How a synthesizer makes sound

Most instruments in music2 are **synthesizers**: they build sound from scratch. Four building blocks show up everywhere, including in the knobs:

- **Oscillator:** makes the raw tone. Each wave shape has its own character: **sine** (pure, soft), **saw** (bright and buzzy, the classic pop lead), **square** (hollow, like old video games). **Unison** or **detune** stacks a few slightly out-of-tune copies for a huge, wide sound.
- **Envelope (ADSR):** shapes the volume over the life of a note. **A**ttack is how fast it starts, **D**ecay how fast it falls after that, **S**ustain the level while the key is held, and **R**elease how long it rings after you let go. A pluck has an instant attack and a quick decay; a pad has a slow attack.
- **Filter:** removes some frequencies. A **low-pass** filter cuts the highs: turning it down makes the sound darker and muffled, turning it up makes it brighter. Sweeping it is the classic build-up.
- **LFO** (low-frequency oscillator): a slow wobble that moves another knob by itself, such as a filter opening and closing in time with the beat.

A **sampler** is the other kind of instrument: instead of building sound, it plays a **recording** (a **sample**) at different pitches.

**In music2:** these blocks are in `std/dsp` (`osc.sine`, `osc.saw`, `osc.square`, `osc.tri`, `adsr`, `svf`), `std/filter` and `std/control` (`lfo`). `synth.custom` has them all in one synth, with unison (up to seven stacked voices), a ladder filter with its own envelope, and drive; `fm.custom` builds sound by FM instead (one sine bending another's pitch, which gives bells and electric pianos). There's no sampler yet ([gap #22](gaps.md#sounds)). Each std instrument is a short function you can read and copy.

## Effects: changing a sound

| Effect | What it does | In music2 |
|---|---|---|
| **Reverb** | The echo of a room or a hall; makes things sound far away and big | `reverb` |
| **Delay** (echo) | Repeats the sound, fading away | `delay` |
| **Chorus** | Slightly detuned copies: wider and dreamier | `chorus` |
| **Distortion** (drive, saturation) | Pushes the sound until it breaks up: gritty, and louder-feeling | `drive`, `bitcrush` |
| **EQ** (equalizer) | Turns ranges of frequencies up or down (more bass, less harshness) | `eq3` |
| **Filter** | A filter used as an effect, good for sweeps and "next room" muffling | `lowpass`, `highpass`, `bandpass`, `notch` |
| **Compressor** | Turns loud moments down, so everything is more even and punchy | `compress` |
| **Limiter** | A very strict compressor at the end that stops anything getting too loud | `limit` |

**In music2:** add an effect with **+** on a box in the Graph view. There are also `phaser`, `tremolo`, `widen`, `gain` and `pan`. If you used the old music2: Space is now `reverb`, Echo is `delay`, Shimmer is `chorus`, Tone is `eq3` and Glue is `compress`. Pump (ducking on every beat) isn't back yet ([gap #21](gaps.md#effects-and-plugins)).

## Mixing: making the parts fit together

- **Volume** is measured in **dB** (decibels). 0 dB means unchanged, −6 dB is about half the level, and +6 dB about double.
- **Pan** places a sound left or right.
- **Mute** silences a track; **solo** plays only that track.
- The **master** is the last stage everything goes through before your speakers.

**In music2:** each track's level (`db`) and `pan` are knobs on the `mixer` box in the Song tab, written in `song.ts`. The `master` is a limiter, so the song never clips. Mute, solo and a mixer panel aren't in the app yet ([gap #4](gaps.md#mixing)).

## More mixing tools

- **Bus** (or group): a track with no notes of its own. Other tracks send their sound into it, and it processes them together, such as one compressor on all the drums.
- **Send:** a copy of a track's sound going into a bus, while the original still goes to the mix. Several tracks can send to one reverb bus, so they share one space.
- **Sidechain:** one track's sound controlling an effect on another. The famous one is **ducking**: every time the kick hits, the bass and pads dip in volume and swell back. That breathing "pump" is the signature sound of dance-pop.
- **Ghost kick:** a kick you don't hear, used only to drive the ducking.
- **Automation:** drawing how a knob changes over time, such as a fade-in, or a filter that opens over 8 bars before the drop.

**In music2:**
- Buses and sends aren't in the app yet ([gap #27](gaps.md#mixing)).
- A sidechain is written in code: `duck(bass.out, kick.env)` in `song.ts`, where `kick.env` is how loud the kick is right now. [`examples/first-song`](../examples/first-song) does this.
- For a ghost kick, use `kick.env` in `duck` but leave the kick out of the `mixer`.
- Automation lanes (`lane.control`) play and show on the timeline, but drawing them in the app isn't there yet ([gap #14](gaps.md#automation)). Today you edit `data/<track>/<lane>.json`, named by the track's and the lane's keys (a key that is a Windows device name, such as `aux`, is written `aux-`).

## Production terms

| Term | Meaning | In music2 |
|---|---|---|
| **Stems** | Separate audio files for each part (drums, bass, vocals…) of a finished song | `m2 bounce`, one track at a time ([gap #36](gaps.md#export)) |
| **Bounce** or **resample** | Record a track's sound into a new audio clip, then change that recording further | `m2 bounce`; see [`examples/resample`](../examples/resample) |
| **Time-stretch** | Change a recording's speed without changing its pitch | `warp` markers on audio clips |
| **Pitch-shift** | Change pitch without changing speed | `pitch` on audio clips (with warp markers) |
| **Warp** | Stretching parts of a recording so it lines up with the beat | `warp` markers on audio clips |
| **Chop** or **slice** | Cut a recording into short pieces and replay them as a new rhythm | Audio clips with an `offset` into the file |
| **Formant** | The "vowel" character of a voice; shifting it makes a voice sound smaller or bigger without changing the note | Not in music2 |
| **Latency** | A delay some effects add; the software has to make up for it so everything stays in time | Plugins that add latency are refused for now ([gap #33](gaps.md#effects-and-plugins)) |
| **True peak** | The real loudest point of the audio, which streaming services check | No meters yet ([gap #20](gaps.md#mixing)) |
