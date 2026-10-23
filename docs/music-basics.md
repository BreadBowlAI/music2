# Music basics

The music ideas music2 uses, in plain words, for someone who has never made a song. Each part ends with where the idea lives in music2. Words about the app itself (track, lane, clip, knob…) are in the manual's [Words you'll see](manual.md#words-youll-see).

**New to all this?** In the app, **Help ▸ Make Your First Song** walks you through a first song in seven steps: drums, a beat, a bass, some bass notes, a listen, a new sound, and an export. Each step ticks itself when you do it. The start screen has the same link: "New to making music? Make your first song". Hold the mouse over any button to see what it does.

## Sound, notes and pitch

- A **sound** is air vibrating. How fast it vibrates is its **pitch**: faster sounds higher.
- A **note** is one sound at one pitch. It starts at one moment and lasts for a while.
- Notes are named with a letter and an octave number. `C4` is "middle C" on a piano, `A4` is the note orchestras tune to, and `C5` is the C one octave higher (it vibrates twice as fast).
- There are 12 notes in an octave: `C C# D D# E F F# G G# A A# B`. On a piano, these are all the white and black keys.
- **Velocity** is how hard a note is played. Harder is louder, and usually brighter.

**In music2:** notes are drawn in the Notes view (the piano roll): double-click to add one, or press B and click. In code, a pitch is a name (`"C4"`) or a number (60 is middle C). Velocity goes from 0 to 1, and a new note gets 0.8; the strip at the bottom of the Notes view sets it.

You can play notes too. ⌘K (Ctrl+K off the Mac) turns the computer's letter keys into a piano: A is middle C. A MIDI keyboard plays the selected track as soon as it is plugged in. Press **Record** (R) to record what you play into a clip: one bar of clicks counts you in first.

## Notes or recordings

This is the most important difference in music software.

| | **Notes** (often called MIDI) | **Audio** (a recording) |
|---|---|---|
| What it is | Instructions: "play C4 at beat 1, for half a beat, fairly hard" | The sound wave itself, like a WAV or MP3 file |
| Like | Sheet music | A recording of someone playing |
| Can you change it? | Completely: any note, or the whole instrument | Mostly as a whole: cut, move, stretch |
| Makes sound by | Being played by an **instrument** | Just playing back |

**In music2:** a notes lane holds clips of notes, and an audio lane holds clips that play a WAV file. Bring a WAV in with **File ▸ Import Audio…** (⇧⌘I), or drag it onto a track or below the tracks. It is copied into the song's `assets/` folder. On the timeline you see its waveform. Move it, trim it, fade it in and out, make it louder or quieter, or play it backwards. To record your voice or a guitar, select an audio track and press R. Stacked takes and punch in are still to come ([gap #25](gaps.md#recording-and-playing-live)).

## Time: tempo, beats, bars and the grid

- **Tempo** is the speed of the song in **BPM** (beats per minute). Dance-pop is usually 118 to 128.
- A **beat** is one pulse: the thing you tap your foot to.
- A **bar** (or measure) is a group of beats. Almost all pop uses **4/4 time**: 4 beats per bar. Song parts are counted in bars ("an 8-bar chorus").
- The **grid** splits beats into smaller steps for placing notes. **1/16** means four steps per beat; that's where hi-hats and fast patterns live. **Quantize** snaps notes onto the grid, and **humanize** nudges them a little so they sound less robotic.

**In music2:** the tempo and the time signature are in the top bar. Drag the tempo up or down, or double-click it and type. Click the time signature to pick another. To change them partway through the song, right-click the ruler. The grid goes from 1/4 to 1/64, and ⌘3 makes it triplets (three notes in the space of two). Right-click a note for **Quantize** (or press Q), **Humanize** and **Swing**. The **metronome** (K) clicks on each beat while the song plays, a higher click on each bar's first beat.

## Key, scale and chords

- A **scale** is a set of 7 of the 12 notes that sound good together. **Major** sounds bright and happy; **minor** sounds emotional and moody. A lot of dance-pop is in minor, which is how it sounds happy and sad at once.
- The **key** is a scale plus its home note, such as "A minor". A minor uses only the white keys: A B C D E F G.
- A **chord** is several notes played together, usually 3 (a **triad**).
- A **chord progression** is the order of chords, usually one per bar, repeating. Chords are named by their place in the scale with Roman numerals. The most famous pop progression is **I–V–vi–IV** (in C major: C, G, Am, F). In minor keys, **i–VI–III–VII** is a classic.

**In music2:** click **Key** in the top bar and pick the song's key, such as A minor. The Notes view then shades the rows that aren't in the key, so the notes that fit stand out. More scales, chord stamps and an arpeggiator are still to come ([gap #16](gaps.md#notes)).

## The parts of a pop song

| Part | What it is | Typical sound |
|---|---|---|
| **Drums** | The rhythm: the **kick** (the boom), the **snare** or **clap** (the crack on beats 2 and 4), the **hi-hats** (the tick-tick in between) | "Four on the floor": a kick on every beat, the heartbeat of disco and house |
| **Bass** | Low notes that lock in with the kick and outline the chords | Often a pumping note between kicks |
| **Chords or pad** | The harmony, a cushion of sound under everything | Lush and held, or short rhythmic **stabs** |
| **Melody, hook or topline** | The tune you sing along to. The **hook** is the catchiest bit | A bright synth lead, or a voice |
| **Counter-melody** | A second tune that answers the melody in its gaps | Plucks, strings |
| **Arp** (arpeggio) | A chord played one note at a time, fast and repeating | Sparkly 16th-note patterns |

**In music2:** **+ Track** (⌘T) lists every sound, drums first. The **Drum kit** plays kick, snare, clap, hats, toms and more from one lane, or pick one drum alone, such as Snare. For the rest: **808 sub** and **Synth bass** (bass), **Supersaw** and **Synth pad** (chords), **Lead** (a hook that glides), **Synth pluck** (arps), **Strings**, **FM keys**, **FM bell** and **FM metal** (metallic hits). To change a track's sound later, right-click it, then **Change instrument…**. In code they are `drums.kit`, `synth.sub` and so on ([`std/src/drums.ts`](../std/src/drums.ts), [`std/src/synth.ts`](../std/src/synth.ts), [`std/src/fm.ts`](../std/src/fm.ts)); [`examples/sounds`](../examples/sounds) uses them all.

## How a synthesizer makes sound

Most instruments in music2 are **synthesizers**: they build sound from scratch. Four building blocks show up everywhere, including in the knobs:

- **Oscillator:** makes the raw tone. Each wave shape has its own character: **sine** (pure, soft), **saw** (bright and buzzy, the classic pop lead), **square** (hollow, like old video games). **Unison** or **detune** stacks a few slightly out-of-tune copies for a huge, wide sound.
- **Envelope (ADSR):** shapes the volume over the life of a note. **A**ttack is how fast it starts, **D**ecay how fast it falls after that, **S**ustain the level while the key is held, and **R**elease how long it rings after you let go. A pluck has an instant attack and a quick decay; a pad has a slow attack.
- **Filter:** removes some frequencies. A **low-pass** filter cuts the highs: turning it down makes the sound darker and muffled, turning it up makes it brighter. Sweeping it is the classic build-up.
- **LFO** (low-frequency oscillator): a slow wobble that moves another knob by itself, such as a filter opening and closing in time with the beat.

A **sampler** is the other kind of instrument: instead of building sound, it plays a **recording** (a **sample**) at different pitches.

**In music2:** these blocks are in `std/dsp` (`osc.sine`, `osc.saw`, `osc.square`, `osc.tri`, `adsr`, `svf`), `std/filter` and `std/control` (`lfo`). `synth.custom` has them all in one synth, with unison (up to seven stacked voices), a ladder filter with its own envelope, and drive; `fm.custom` builds sound by FM instead (one sine bending another's pitch, which gives bells and electric pianos). std's **Sampler** plays a WAV file on each note, higher or lower by the note; loop points and velocity layers are still to come ([gap #22](gaps.md#sounds)). Each std instrument is a short function you can read and copy.

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

**In music2:** add an effect with **+** on a box in the Device view (⌥1), or drag one from the side browser onto a track. Its **⋯** menu turns it off and on, or moves it before or after its neighbour. There are also `phaser`, `tremolo`, `widen`, `gain` and `pan`. If you used the old music2: Space is now `reverb`, Echo is `delay`, Shimmer is `chorus`, Tone is `eq3` and Glue is `compress`. Pump (ducking on every beat) isn't back yet ([gap #21](gaps.md#effects)).

## Mixing: making the parts fit together

- **Volume** is measured in **dB** (decibels). 0 dB means unchanged, −6 dB is about half the level, and +6 dB about double.
- **Pan** places a sound left or right.
- **Mute** silences a track; **solo** plays only that track.
- The **master** is the last stage everything goes through before your speakers.

**In music2:** each track's volume and pan are a fader and a pan knob in the Mix view (⌥3, or Alt+3 off the Mac), written into the `mixer` in `song.ts`. The master has a fader too, then a limiter, so the song never clips. **M** and **S** on each track mute and solo it while you listen (press M or S for the selected track); the export still has every track. **Mute in song**, in a track's menu, leaves it out of the export too. Every track has a level meter in its header and on its Mix strip.

## More mixing tools

- **Bus** (or group): a track with no notes of its own. Other tracks send their sound into it, and it processes them together, such as one compressor on all the drums.
- **Send:** a copy of a track's sound going into a bus, while the original still goes to the mix. Several tracks can send to one reverb bus, so they share one space.
- **Sidechain:** one track's sound controlling an effect on another. The famous one is **ducking**: every time the kick hits, the bass and pads dip in volume and swell back. That breathing "pump" is the signature sound of dance-pop.
- **Ghost kick:** a kick you don't hear, used only to drive the ducking.
- **Automation:** drawing how a knob changes over time, such as a fade-in, or a filter that opens over 8 bars before the drop.

**In music2:**
- Buses and sends: in **Mix** (⌥3), **Add bus** makes a Reverb, a Delay or a Group. Turn up a track's knob for a bus to send to it; set a track's output to a Group to put it through the Group's fader.
- A sidechain is written in code: `duck(bass.out, kick.env)` in `song.ts`, where `kick.env` is how loud the kick is right now. [`examples/first-song`](../examples/first-song) does this.
- For a ghost kick, use `kick.env` in `duck` but leave the kick out of the `mixer`.
- Automation: right-click a knob, then **Automate**. It gets a lane on the timeline. Double-click the lane for a clip, then click its line to add points, or press B and paint. Curves between points are still to come ([gap #14](gaps.md#automation)).

## Production terms

| Term | Meaning | In music2 |
|---|---|---|
| **Stems** | Separate audio files for each part (drums, bass, vocals…) of a finished song | **File ▸ Export…**, then **Stems**: each track as its own WAV file, before its volume and pan. Also `m2 bounce`, one track at a time |
| **Bounce** or **resample** | Record a track's sound into a new audio clip, then change that recording further | `m2 bounce`; see [`examples/resample`](../examples/resample) |
| **Time-stretch** | Change a recording's speed without changing its pitch | `warp` markers on audio clips |
| **Pitch-shift** | Change pitch without changing speed | `pitch` on audio clips (with warp markers) |
| **Warp** | Stretching parts of a recording so it lines up with the beat | `warp` markers on audio clips |
| **Chop** or **slice** | Cut a recording into short pieces and replay them as a new rhythm | Audio clips with an `offset` into the file |
| **Formant** | The "vowel" character of a voice; shifting it makes a voice sound smaller or bigger without changing the note | Not in music2 |
| **Latency** | A delay some effects add; the software has to make up for it so everything stays in time | music2 makes up for a plugin's latency, so every track stays in time, live and in exports |
| **True peak** | The real loudest point of the audio, which streaming services check | Not measured yet. The meters show each track's level, red at 0 dB ([gap #20](gaps.md#mixing)) |
