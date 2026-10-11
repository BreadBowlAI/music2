# music2 — how it works

music2 is a full music studio (a DAW, like Ableton Live or Logic Pro), with every tool
visible from the start. It's part of **suite2**, alongside film2 and dj2. There's no AI built
in, but everything it does is a scriptable command, so an assistant running on your machine
(like a Claude Code session) or any script can drive it. This document explains it from zero:

1. **[Music in ten minutes](#part-1--music-in-ten-minutes)**: every music concept music2 uses, in plain words.
2. **[Plugins, instruments, effects, devices](#part-2--plugins-instruments-effects-devices)**: the vocabulary, precisely.
3. **[The architecture](#part-3--the-architecture-from-scratch)**: how the software is built, step by step.
4. **[Scripting music2](#part-4--scripting-music2)**: driving the app from Claude Code or any script.
5. **[The plugin standard (m2plug)](#part-5--the-plugin-standard-m2plug)**: how anyone can publish a sound.
6. **[Where it's going](#part-6--where-its-going-production-quality)**: the plan to reach professional pop production.

If you only read one section, read Part 2.

---

## Part 1 — Music in ten minutes

### Sound, notes and pitch

A **sound** is air vibrating. How fast it vibrates is its **pitch**: faster sounds higher.
A **note** is one sound at one pitch, starting at one moment and lasting for a while.
Notes are named with letters and an octave number: `C4` is "middle C" on a piano, `A4` is the
note orchestras tune to, and `C5` is the C one octave higher (twice as fast a vibration).
There are 12 notes per octave: `C C# D D# E F F# G G# A A# B`. On a piano these are all the
white and black keys.

**Velocity** is how hard a note is played (1–127). Higher velocity is louder and usually brighter.

### MIDI vs audio: instructions vs recordings

This is the most important distinction in music software.

| | **MIDI** (notes) | **Audio** (recording) |
|---|---|---|
| What it is | Instructions: "play C4 at beat 1, hold for half a beat, medium-hard" | The actual sound wave, like an MP3 or WAV |
| Analogy | Sheet music | A recording of a performance |
| Editable? | Completely: change any note, swap the instrument | Mostly as a whole: cut, move, stretch |
| Makes sound by | Being played by an **instrument** | Just playing back |

In music2, a **MIDI clip** holds notes, and an **audio clip** holds a recording, such as an
imported sound effect or vocal.

### Time: tempo, beats, bars, grid

- **Tempo** is the speed of the song in **BPM** (beats per minute). Dance-pop is usually 118–128.
- A **beat** is one pulse, the thing you tap your foot to.
- A **bar** (or measure) is a group of beats. Almost all pop uses **4/4 time**: 4 beats per
  bar. Song sections are counted in bars ("an 8-bar chorus").
- The **grid** divides beats into smaller steps for placing notes. **1/16** means four steps
  per beat, which is where hi-hats and fast patterns live. **Quantize** snaps notes onto the
  grid; **humanize** nudges velocities so it sounds less robotic.

### Key, scale, chords

- A **scale** is a set of 7 notes (out of 12) that sound good together. **Major** sounds
  bright and happy; **minor** sounds emotional and moody. Lots of dance-pop is in minor,
  which is how it sounds euphoric and sad at the same time.
- The **key** is a scale plus its home note, e.g. "A minor". The piano roll
  can **scale-lock** ("In-key only"): hide every note outside the key, so you can't play a
  "wrong" one. It's off by default.
- A **chord** is several notes played together (usually 3: a **triad**).
- A **chord progression** is the sequence of chords, usually one per bar, repeating. Chords
  are named by their position in the scale with Roman numerals. The most famous pop
  progression is **I–V–vi–IV** (in C major: C, G, Am, F). In minor keys, **i–VI–III–VII** is
  a classic.

### The parts of a pop song

| Part | What it is | Typical sound |
|---|---|---|
| **Drums** | The rhythm. **Kick** (the boom), **snare/clap** (the crack on beats 2 and 4), **hi-hats** (the tick-tick between) | "Four on the floor" = a kick on every beat, the disco/house heartbeat |
| **Bass** | Low notes that lock with the kick and outline the chords | Often an offbeat "pump" between kicks |
| **Chords / pad** | The harmony, a cushion of sound under everything | Lush, sustained, or short rhythmic **stabs** |
| **Melody / hook / topline** | The tune you sing along to. The **hook** is the catchiest bit | Bright synth lead or vocal |
| **Counter-melody** | A second tune that answers the melody in its gaps | Plucks, strings |
| **Arp** (arpeggio) | A chord played one note at a time, fast and repeating | Sparkly 16th-note patterns |

### How synthesizers make sound

Most music2 instruments are **synthesizers**: they build sound from scratch. Four building
blocks show up everywhere, including in the knobs:

- **Oscillator**: makes the raw tone. Waveforms have different characters: **sine** (pure,
  soft), **saw** (bright, buzzy, the classic trance/pop lead), **square** (hollow, video-gamey).
  **Unison/detune** stacks several slightly out-of-tune copies for a huge, wide sound (a
  **supersaw**).
- **Envelope (ADSR)**: shapes volume over the life of a note. **A**ttack (how fast it starts),
  **D**ecay (how fast it falls after the start), **S**ustain (the level while held),
  **R**elease (how long it rings after you let go). A pluck has an instant attack and quick
  decay. A pad has a slow attack.
- **Filter**: removes some frequencies. A **low-pass** filter cuts the highs, so turning it
  down makes the sound darker and muffled, and turning it up makes it brighter. Sweeping it is
  the classic "build-up" effect.
- **LFO** (low-frequency oscillator): a slow wobble that moves another knob automatically,
  like vibrato or a slowly opening filter.

A **sampler** is the other kind of instrument: instead of synthesizing, it plays a
**recording** (a **sample**) at different pitches.

### Effects: changing sound

| Effect | What it does | music2 plugin |
|---|---|---|
| **Reverb** | Room/hall echo; makes things sound far away and big | Space |
| **Delay / echo** | Repeats the sound in time with the song | Echo |
| **Chorus** | Slightly detuned copies: wider, dreamier | Shimmer |
| **Distortion / drive / saturation** | Pushes the sound until it breaks up: gritty, louder-feeling | Drive |
| **EQ** (equalizer) | Turns frequency ranges up or down (more bass, less harshness) | Tone |
| **Filter** | A low-pass filter as an effect, good for sweeps and "next room" muffling | Filter |
| **Compressor** | Turns loud moments down so everything is more even and punchy | Glue |
| **Limiter** | A very strict compressor at the end that stops anything getting too loud | (inside Master) |

### Mixing: making the parts fit together

- **Volume** is measured in **dB** (decibels). 0 dB = unchanged, −6 dB ≈ half as loud,
  +6 dB ≈ twice as loud.
- **Pan** places a sound left or right in the stereo field.
- **Mute** silences a track. **Solo** plays only that track (and anything it feeds).
- The **master** is the final stage everything flows into before your speakers. music2's
  master chain gently compresses and limits so the song sounds finished and never distorts.

### Advanced mixing (the Track inspector)

These are the tools professional producers use. All of them exist in music2 today.

- **Bus** (also called a group): a track with no notes of its own. Other tracks send their
  sound *into* it, and it processes them together. Example: route all drum tracks into a
  "Drums" bus and put one compressor on the bus so they gel.
- **Output**: where a track's sound goes. Normally the master; set it to a bus to group the
  track.
- **Send**: a *copy* of a track's sound into a bus, at an adjustable level, while the original
  still goes to its output. Example: send several tracks into one "Reverb" bus to put them all
  in the same space, with one reverb instead of five. **Pre-fader** sends ignore the track's
  volume knob; **post-fader** sends follow it.
- **Sidechain**: letting one track's sound control an effect on *another* track. The most
  famous use is **ducking**: every time the kick hits, the bass and pads automatically dip in
  volume and swell back. That breathing "**pump**" is the signature sound of dance-pop and
  French house. In music2, add the **Duck** effect to a track and pick the kick as its
  sidechain source.
- **Ghost kick**: a muted track that only exists to drive sidechain ducking, so you can have
  the pump without hearing an extra kick. music2's sidechain listens *before* the volume
  fader, so muted tracks still work as sources.
- **Automation**: drawing how a knob changes over time, for example a volume fade-in, a
  filter that opens over 8 bars before the drop, or a reverb that swells at the end of a
  phrase. In music2, open a track's automation lane (the pulse icon), pick a knob, and click
  to draw points.

### Production terms (coming next)

| Term | Meaning |
|---|---|
| **Stems** | Separate audio files for each part (drums, bass, vocals…) of a finished song. Labels, remixers and mixing engineers ask for them |
| **Bounce / resample** | Record a track's output into a new audio clip, then mangle that recording further. A core hyperpop technique |
| **Time-stretch** | Change a recording's speed without changing its pitch |
| **Pitch-shift** | Change pitch without changing speed |
| **Formant** | The "vowel character" of a voice. Shifting it independently of pitch makes a voice sound smaller/chipmunky or bigger without changing the note. The PC Music vocal sound |
| **Warp** | Stretching parts of a recording so it lines up with the song's beat |
| **Chop / slice** | Cut a recording (often a vocal) into short pieces and replay them as a new rhythm |
| **Latency** | Delay that some effects add; the software has to compensate so everything stays in time |
| **Aliasing / oversampling** | Harsh digital artifacts from distortion at high pitches; oversampling processes at a higher resolution internally to avoid them |
| **True peak** | The real loudest point of the audio after it's converted to sound, which streaming services check |

---

## Part 2 — Plugins, instruments, effects, devices

These four words get mixed up constantly, in music software generally and in music2.
Here's exactly what each means in music2.

### The short version

```text
PLUGIN   = a recipe file that says how to make or change sound   (e.g. "Pluck", "Space")
           three kinds: instrument, kit, effect

DEVICE   = a plugin placed on a track, with its own knob settings (your Pluck on the Hook track)

INSTRUMENT = the one device at the start of a track: turns NOTES into SOUND
EFFECT     = devices after the instrument: take SOUND in, give changed SOUND out
```

### Plugin: a recipe

A **plugin** is a file that describes a sound-making or sound-changing machine. In music2 it's
a small JSON file in the **m2plug** format (Part 5). It's like a recipe in a cookbook: it says
which building blocks (oscillators, filters…) to use and how to wire them, and which **knobs**
(called **macros**) the user gets.

Every plugin is one of three **kinds**:

| Kind | Input → output | Examples in music2 |
|---|---|---|
| **instrument** | notes → sound | Pluck, Supersaw Lead, Glow Pad, Pump Bass, Electric Keys, Disco Strings, Sampler |
| **kit** | notes → sound, but each note is a *different* sound | Pop Kit (C2 = kick, D2 = snare, D#2 = clap, F#2 = hi-hat, …) |
| **effect** | sound → changed sound | Space, Echo, Shimmer, Drive, Tone, Filter, Glue, Duck, Pump, Master |

A **kit** is really a special instrument for drums: instead of one sound played at different
pitches, each key triggers its own sound (kick, snare, clap, hat…). It's a separate kind
because the piano roll then shows drum names instead of note names.

The **Sampler** is an instrument whose sound comes from a recording instead of synthesis.
**Make instrument** on any sound in the Sounds panel creates a Sampler device loaded with that
recording, so you can play melodies with it.

### Device: a plugin in use

A **device** is a plugin *placed on a track*, together with its settings: knob values, which
recording a Sampler uses, whether it's switched off (**bypass**), and its sidechain source.
The same plugin can be used as many devices: two tracks can both use "Pluck", one dark and
one bright. It's the recipe vs the dish.

In the song file a device looks like this:

```json
{ "id": 12, "plugin": "m2/pluck", "macros": { "tone": 0.8 }, "bypass": false, "sidechain": null }
```

### Instrument and effects on a track: the chain

Every track is a chain of devices that sound flows through, left to right:

```text
MIDI clip notes ──▶ [ INSTRUMENT ] ──▶ [ effect ] ──▶ [ effect ] ──▶ volume/pan ──▶ output
                     Pump Bass          Drive          Duck
                                                       ▲
                                    sidechain from ────┘ Drums track
```

- An **instrument track** has exactly one instrument (or kit) plus any number of effects.
- An **audio track** has no instrument. Its clips are already sound, so it's just effects.
- A **bus** has no instrument and no clips. Its "input" is whatever other tracks send into it.

### How this compares to Ableton / Logic

In traditional music software, "plugin" means a compiled program in a format like **VST**,
**AU** or **CLAP** that the app loads and runs. Plugins can be instruments or effects, the
same split as here. The difference is what a plugin *is*:

| | VST / AU / CLAP | music2 m2plug |
|---|---|---|
| What it is | Compiled code (a binary program) | A JSON description of a graph of built-in building blocks |
| Safety | Can do anything your computer can; a buggy one crashes the app | Can't run code; at worst it sounds bad |
| Sharing | Installers, licenses, per-OS builds | Push one file to GitHub |
| Readable by people and tools | No | Yes: any plugin can be read, diffed and explained |
| Power | Unlimited | Limited to the built-in blocks (escape hatches planned, Part 6) |

---

## Part 3 — The architecture, from scratch

### 3.1 The big picture

```text
 ┌──── the app window (Tauri web view) ─────┐
 │  React UI: timeline, piano roll, panels  │
 └───────┬───────────────────────▲──────────┘
         │ commands (HTTP)       │ song changes, playhead, meters (WebSocket)
 ┌───────▼───────────────────────┴──────────────────────────────────────────┐
 │  music2 engine (one Rust program on your machine)                        │
 │                                                                          │
 │   Session ── the song + undo history + sounds on disk                    │
 │     ▲                                                                    │
 │     │ every change is a COMMAND ("clip.add", "send.set", …)             │
 │     ├──── from the UI                                                    │
 │     ├──── from scripts / Claude Code (HTTP API, ~/.music2/session.json)  │
 │     └──── from the command line (music2-cli)                             │
 │                                                                          │
 │   Renderer ── turns the song into sound ──▶ speakers / WAV export        │
 └──────────────────────────────────────────────────────────────────────────┘
```

music2 is one native Mac app (`music2-desktop`, built with Tauri). The window shows the React
UI in the system's web view, but that UI is only a remote control and a screen. The **engine**
holds the song and makes the sound, and sound goes straight from the engine to your speakers,
which keeps latency low and makes playback identical to export.

The engine runs inside the app on a private port, and every request must carry a token that
changes each launch, so web pages and other programs can't control it (local tools you run get
the token from `~/.music2/session.json`, see Part 4). The app's Rust side provides the menu bar,
real open/save dialogs, the "unsaved changes" prompt, files dragged in from Finder, and OS
right-click menus. File actions run natively and call the engine directly; the UI then updates
over the same live connection as after any other edit.

### 3.2 The song (the data model)

A song is plain data, saved as a JSON file (`.m2song`) in `~/Music/music2/`:

```text
Song
├── title, tempo (bpm), time signature, key
├── tracks[]
│   └── Track
│       ├── kind: instrument | audio | bus
│       ├── instrument: Device (instrument tracks)
│       ├── effects: Device[]
│       ├── clips[]
│       │   └── Clip: start, length, and either
│       │       ├── MIDI: notes[]  → Note { pitch, start, length, velocity }
│       │       └── Audio: which recording, where in it to start
│       ├── volume (dB), pan, mute, solo
│       ├── output: master or a bus
│       ├── sends[]: { to: bus, level, pre/post-fader }
│       └── automation[]: { which knob, points over time }
├── master: volume + effect devices
└── assets[]: recordings (generated or imported WAVs) used by clips and samplers
```

Design decisions, and why:

- **Time is counted in "ticks", 960 per beat.** Whole numbers avoid rounding drift, and 960
  divides evenly into halves, thirds, quarters, fifths, sixths and eighths, so every common
  rhythm (including triplets) is exact. People and scripts speak bars and beats; the engine
  converts.
- **Everything has a permanent id**, every track, clip, note, device and automation lane. The
  UI, undo, sync and scripts refer to "note 412", never "the 5th note", so nothing gets mixed up
  when things are added, deleted or reordered.
- **Unchanged parts are shared between versions.** When you edit one note, the new version of
  the song reuses every untouched track and clip from the previous version instead of copying
  them. Keeping 200 versions for undo costs almost nothing, and spotting "what changed" is
  instant. (Technically: tracks and clips are reference-counted `Arc`s, and edits copy only
  the path they touch.)
- **Files carry a format number** (currently 3). Older songs are upgraded automatically when
  opened.

### 3.3 Commands: the one way to change anything

Every edit, no matter who makes it, is a **command**: a name plus parameters.

```json
{ "cmd": "clip.add", "params": { "track": "Bass", "start_bar": 1, "length_bars": 4 } }
```

| Group | Commands |
|---|---|
| Song | `song.set` (title, tempo, key), `song.describe` (read) |
| Tracks | `track.add` (instrument, audio or bus), `track.remove`, `track.set` (name, volume, pan, mute, solo, output) |
| Devices | `track.setInstrument`, `effect.add`, `effect.remove`, `effect.bypass`, `device.setMacros` (turn knobs), `device.setSidechain` |
| Routing | `send.set`, `send.remove` |
| Automation | `automation.set`, `automation.clear` |
| Clips | `clip.add`, `clip.remove`, `clip.set` (move/resize), `clip.duplicate` |
| Notes | `notes.add`, `notes.set`, `notes.remove`, `notes.transpose`, `notes.quantize`, `notes.velocity`, `clip.notes` (read) |
| Sounds | `audio.place`, `sampler.load`, `asset.setRoot` |
| Master | `master.set` |

Each command declares its parameters with a JSON Schema. That buys four things at once:

1. **One undo history.** A command produces a new version of the song, and the old version
   goes on the undo stack. Multi-step actions (a drag, a scripted recipe) are **grouped** into
   one undo step.
2. **Self-describing.** `GET /api/commands` (or `music2-cli commands --json`) lists every
   command with its schema, so a script or an assistant can discover what's possible. Every new
   feature is immediately scriptable.
3. **Every interface is equal.** The UI, the CLI and outside scripts all go through the same door.
4. **Safety.** After every command the song is validated (ids valid, no routing loops, no
   clips on buses). Invalid changes are rejected and nothing changes.

The UI sends commands as a **single ordered queue**, so the engine always sees them in
the order you made them. Without that, an "undo group starts" message could overtake the edit
it belongs to.

### 3.4 From notes to sound: the renderer

The **renderer** turns a song into sound. It works in tiny **blocks** of 64 samples (about
1.3 ms) at a time. For each block:

```text
1. Decide the processing order.
     Sidechain sources come before the devices they control,
     and every track comes before the bus it feeds.          (a "topological sort")

2. For each track, in that order:
     a. Source sound
          instrument track → play the notes that start/stop in this block
          audio track      → read the recordings at this point in time
          bus              → sum everything routed into it
     b. Automation        → set knobs/volume/pan to their drawn values for this moment
     c. Effects           → run each effect device; a device with a sidechain
                            also gets the source track's sound
     d. Pre-fader sends   → copy into buses
     e. Volume and pan    → the "fader"
     f. Output            → into its bus or the master; post-fader sends copy into buses

3. Master chain (compressor + limiter) and master volume → speakers or file
```

Inside each device, a **plugin graph** (Part 5) does the work. Instruments run one copy of
their graph per sounding note (**voices**, up to 12 at once by default); kits run one per drum sound.

Rules that keep it reliable:

- **Playback and export are the same code**, so an exported WAV sounds exactly like what you
  heard.
- **Deterministic**: the same song always produces the same audio, regardless of how the
  sound card chops up time. Tests check this.
- **The audio thread never waits.** When you edit, a new renderer is prepared off to the side
  and swapped in. Devices whose structure didn't change hand their live state (ringing notes,
  reverb tails) to the new one, so editing during playback doesn't glitch.
- **Routing loops are impossible**: a command that would create one (bus A → bus B → bus A)
  is rejected.

### 3.5 Keeping the screen in sync

After the UI connects and receives the song once, the engine sends only **patches**:
what changed since last time. Because unchanged tracks are shared between versions (3.2), the
engine skips them instantly. Measured on a large song (~690 KB of data):

| Edit | Sent to the UI |
|---|---|
| Move a clip | ~150 bytes |
| Edit one note | 84 bytes |
| Draw a note | ~400 bytes |

The UI applies patches the same way, reusing everything unchanged, so only the edited
track redraws.

### 3.6 Code layout

The code is split into layers (borrowed from FilmCraft, see `filmcraft-architecture.md`). Each
layer may only use layers below it, which keeps the core reusable by every frontend.

| Layer | Crate (folder) | What it does |
|---|---|---|
| Apps | `apps/music2-desktop` | The native app (Tauri). Runs the engine in-process on a private port with a per-launch token, shows the UI in a native window, and adds the menu bar, file dialogs, Finder drag-and-drop and OS right-click menus |
| | `apps/music2-cli` | The command-line tool (`exec`, `import`, `render`, `plugin add`) |
| Frontends | `crates/server` | HTTP + WebSocket API for the web UI; patch sync |
| | `crates/audio-io` | Talks to the sound card |
| | `web/` | The React + Tailwind UI |
| Engine | `crates/engine` | Session, commands, undo, plugin install |
| Processing | `crates/render` | Song → sound (playback and export) |
| Plugins | `crates/graph` | The m2plug standard: load, check, and run plugin graphs |
| Models | `crates/project` | The song data model and file format |
| | `crates/dsp` | Sound building blocks (oscillators, filters, reverb…) |
| Foundations | `crates/time` | Ticks, tempo, bars/beats math |
| | `crates/theory` | Keys, scales, chords, note names |

---

## Part 4 — Scripting music2

music2 has no AI inside it. Instead, the engine's API is open to programs you run yourself, so
a Claude Code session in your terminal (or any script) can read and edit the song that's open
in the app, and you see every change live, with normal undo.

**Finding the app.** When the desktop app starts, it writes `~/.music2/session.json`:

```json
{ "url": "http://127.0.0.1:55085", "token": "…", "pid": 72772 }
```

The file is readable only by your user account. Every `/api` request needs the token in an
`x-m2-token` header.

**The endpoints that matter:**

| Request | What it does |
|---|---|
| `GET /api/commands` | Every command with a description and JSON Schema |
| `GET /api/describe` (`?clip=ID`) | A compact summary of the song: tracks, devices, clips (and one clip's notes) |
| `POST /api/exec` `{"cmd", "params"}` | Run one command (one undo step) |
| `POST /api/steps` `{"label", "steps": [[cmd, params], …]}` | Run several commands as one undo step; `"track": "$new"` refers to the track the recipe just created |
| `POST /api/import-path` `{"path", "bar", "as": "clip" \| "instrument"}` | Import a WAV file as a clip at a bar, or as a Sampler instrument |
| `POST /api/undo`, `/api/redo` | Undo / redo |
| `POST /api/transport` `{"action": "play" \| "stop" \| "toggle" \| "seek", "tick"}` | Playback |
| `POST /api/song/save`, `/api/song/export` | Save, or render to WAV |

For example:

```bash
S=~/.music2/session.json; URL=$(jq -r .url $S); TOKEN=$(jq -r .token $S)
curl -s -H "x-m2-token: $TOKEN" $URL/api/describe
curl -s -H "x-m2-token: $TOKEN" -H 'content-type: application/json' $URL/api/exec \
  -d '{"cmd":"clip.add","params":{"track":"Drums","start_bar":1,"length_bars":1,
       "notes":[{"pitch":39,"start":1,"length":0.5},{"pitch":39,"start":3,"length":0.5}]}}'
```

With the app closed, `music2-cli exec` runs the same commands directly on a `.m2song` file.

**Sound generation** (text-to-audio through fal.ai's Stable Audio) was removed for now. If it
comes back, it belongs here as an outside tool: generate a WAV, then import it as a clip or a
Sampler instrument.

---

## Part 5 — The plugin standard (m2plug)

### What's inside a plugin

A plugin is a **graph**: boxes (**nodes**) connected by wires, where sound flows along the
wires. Here's the Pluck instrument:

```text
note pitch ──▶ [osc: saw ×3] ──▶ [filter] ──▶ [vca] ──▶ out
                                    ▲            ▲
          [env: quick]  ────────────┘            │   (envelope opens the filter)
          [env: amp]    ─────────────────────────┘   (envelope shapes the volume)
```

and here's the file (slightly trimmed; the real one also has a Width knob):

```json
{
  "m2plug": "0.1",
  "id": "m2/pluck",
  "name": "Pluck",
  "kind": "instrument",
  "description": "Short, bright, slightly detuned pluck. Great for chord stabs and arps.",
  "macros": [
    {"id": "tone",  "name": "Tone",  "default": 0.55},
    {"id": "decay", "name": "Decay", "default": 0.35}
  ],
  "voice": {
    "nodes": [
      {"id": "osc",  "type": "osc",    "params": {"wave": "saw", "unison": 3, "detune": 12}},
      {"id": "fenv", "type": "env",    "params": {"a": 0.001, "d": {"macro": "decay", "from": 0.06, "to": 0.6}, "s": 0, "r": 0.15}},
      {"id": "aenv", "type": "env",    "params": {"a": 0.002, "d": {"macro": "decay", "from": 0.15, "to": 1.6}, "s": 0, "r": 0.2}},
      {"id": "flt",  "type": "filter", "params": {"mode": "lp", "cutoff": {"macro": "tone", "from": 300, "to": 5000, "curve": "exp"}, "mod_amount": 3},
       "inputs": {"in": "osc", "mod": "fenv"}},
      {"id": "amp",  "type": "vca",    "params": {"gain": 0.62}, "inputs": {"in": "flt", "cv": "aenv"}}
    ],
    "out": "amp"
  }
}
```

- **`kind`** is `instrument`, `kit` or `effect` (Part 2).
- **Instruments** describe one **voice** (the graph for a single note); the engine runs a copy
  per note. They may add a **`post`** graph that processes all voices together (e.g. one shared
  reverb).
- **Kits** list **pads**: `{pitch, name, voice}`, one sound per key.
- **Effects** have one **`graph`** that reads the incoming sound from the `in` source.
- **Macros** are the knobs users see. A node parameter can be bound to a macro with a range:
  `{"macro": "tone", "from": 300, "to": 5000, "curve": "exp"}` means "as the Tone knob goes
  from 0 to 1, the cutoff goes from 300 Hz to 5000 Hz".

**Sources** a graph can read:

| Source | What it is | Available in |
|---|---|---|
| `note.freq`, `note.gate`, `note.velocity` | The note being played (pitch, held or not, how hard) | instrument and kit voices |
| `in` | The incoming sound | effects, `post` graphs |
| `sidechain` | Another track's sound, chosen by the user (falls back to `in`) | effects, `post` graphs |

**Built-in nodes** (the building blocks):

| Node | Plain-words job |
|---|---|
| `osc` | Makes a tone: sine/saw/square/triangle, with unison/detune and pitch modulation |
| `noise` | Makes hiss (the basis of hi-hats, snares, risers) |
| `env` | An ADSR envelope (a shape over a note's life) |
| `lfo` | A slow wobble, optionally in time with the song |
| `const`, `map` | A fixed value; scale-and-offset a signal (e.g. turn an envelope into a pitch sweep for a kick) |
| `vca`, `mix`, `gain`, `pan` | Volume control, summing, gain in dB, left/right placement |
| `filter` | Low/high/band-pass filter with resonance and modulation |
| `eq` | Three-band tone control plus low cut |
| `drive` | Saturation/distortion |
| `delay`, `reverb`, `chorus` | Echo, room, width |
| `comp` | Compressor, with an optional sidechain input |
| `pump` | Tempo-synced ducking without a sidechain (the "fake" pump) |
| `limit` | Limiter |
| `sampler` | Plays a recording, pitched by the note |

**Safety**: a plugin can only wire these built-in nodes together, so loading one can never run
code. Every plugin is test-compiled and test-played when installed; broken ones are rejected.

### Built-in library

**Instruments:** Pluck, Supersaw Lead, Glow Pad, Pump Bass, Electric Keys, Disco Strings,
Sampler. **Kit:** Pop Kit. **Effects:** Space (reverb), Echo (delay), Shimmer (chorus),
Drive, Tone (EQ), Filter, Glue (compressor), Duck (sidechain compressor), Pump, Master.

### Publishing a plugin

Put the JSON file in a GitHub repo as `m2plug.json` (or several as `plugins/*.m2plug.json`):

```bash
music2-cli plugin check my-pad.m2plug.json          # validate before publishing
music2-cli plugin add github:you/m2-glitter-pad     # anyone can install it
```

The installed plugin's id becomes `github:you/m2-glitter-pad`, and songs record that id, so a
song that uses it knows where to get it. No store, no accounts.

---

## Part 6 — Where it's going: production quality

The goal is release-quality pop stems in the PC Music / A.G. Cook lineage: extreme sound
design, chopped and pitched vocals, resampling, automation everywhere, and a pristine final
mix. The plan, in order:

| Step | What | Status |
|---|---|---|
| 1 | **Data model**: permanent ids on everything, shared unchanged parts between versions | ✅ Done |
| 2 | **Project graph**: buses, outputs, sends, real sidechain (Duck), automation of volume, pan and any knob | ✅ Done |
| 3 | **Audio manipulation**: time-stretch, pitch and formant per clip; warp; slicing/chopping; bounce-to-audio (resample); **stem export** | Next |
| 4 | **Plugin escape hatches**: custom code nodes compiled to WebAssembly (still sandboxed), then hosting **CLAP** plugins so producers can use Vital, Serum, FabFilter | Planned |
| 5 | **Sound-quality pass**: oversampled distortion (less aliasing), a better reverb, true-peak limiting, latency compensation, sample-exact automation, tempo changes | Planned |

**Known limits right now:**

- Automation updates every 64 samples (~1.3 ms) and is smoothed. That's fine for fades and
  sweeps, but not exact enough for very sharp stutter or gate effects.
- There's no latency compensation yet. Nothing built-in adds latency today, but future
  lookahead effects will need it.
- The tempo is constant (no speed-ups or slow-downs within a song).
- Audio import is WAV only.

The idea that ties it together: **the project is one graph.** Instruments and effects are
graphs; tracks, buses, sends, sidechains and automation are wires between them. Every new
capability arrives as new commands, which means it's scriptable the day it ships.

### Principles that don't change

1. **A complete studio, all visible.** No beginner mode and no hidden tools: mixing, routing
   and automation are always there.
2. **Every edit is a command.** One way in for the UI, the CLI and outside scripts, with one
   undo history.
3. **Open to automation, nothing built in.** Assistants and scripts drive music2 from outside
   through the same commands; the app itself makes no AI calls.
4. **Plugins are data.** Safe, shareable, readable.
5. **No historic baggage** unless it earns its place. For example, there's no separate
   "aux return" track type: a bus is just a track other tracks feed.
