# Sounds that make notes: note effects

| | |
|---|---|
| **Status** | Proposed, for the owner's yes. Nothing here is built. |
| **Written** | 2026-11-07, from `main` at `c653658c` |
| **Serves** | gap #16 (an arpeggiator, scales in code). It touches gap #24 (recording the wheels and the pedal) and the Deferred rows for event outputs and for events made by code ([ABI-2](../../architecture.md#kernel-abi-reservations), [ABI-4](../../architecture.md#kernel-abi-reservations)) |
| **Mockup** | `$SCRATCH/events-note/mockup.html`: the Arpeggiator's card before a Pluck. Hold keys on its keyboard and the steps light up; it can beep. |

**In short**
- **Today notes go straight from a lane to an instrument.** Nothing can stand between them. So music2 has no arpeggiator, no chord tool, no note echo and no scale helper, which every DAW a pop producer uses has.
- **A note effect takes notes in and gives notes out.** It sits before the instrument on a track's chain: keys → Arpeggiator → Chord → Pluck → Reverb. You add it from **+ Note effect**, as you add an effect after a sound.
- **It is a sound that plays, not a tool that writes notes.** It runs as the song plays, to the sample, so an arpeggiator follows the keys you hold on a MIDI keyboard. An export sounds the same as playback.
- **The core gains one thing: a kernel may give notes.** The kernel ABI reserved this shape in step 9 ([ABI-2](../../architecture.md#kernel-abi-reservations)); building it keeps kernel ABI v1. Around it: any notes input may read a note effect's notes, a guard in the host so no note ever hangs, and (if D4 is yes) the song's tempo as a signal. No new std kernel, and no other new word in `@m2/core`.
- **TypeScript alone can't do it.** Song code runs once, at build, and may not read the notes in the lanes ([RENDER-15](../../architecture.md#events)). It can't hear a key pressed while the song plays. Only code that runs on the audio thread can.
- **Every note effect is a pack.** The first, `@m2/notes`, has four: **Arpeggiator**, **Chord**, **Note echo** and **Scale**. Generators written in TypeScript (a Euclid rhythm, a pattern like Strudel's) come later as a pack too, on the same primitive.
- **Bake** turns what a note effect plays into notes in a clip, which you can then edit. That is how you keep an arpeggio you like.

## Words used here

| Word | Meaning |
|---|---|
| **Note effect** | A sound that takes notes and gives notes, before the instrument. Ableton calls these MIDI effects, Logic MIDI FX, Bitwig Note FX. |
| **Arpeggio** | A chord played one note at a time, in a pattern, over and over. An **arpeggiator** makes one from the keys you hold. |
| **Held notes** | The notes that are on right now: a chord drawn in a clip, or keys you hold down. |
| **Step** | One note of an arpeggio. The **rate** is how long a step is: 1/16 is a sixteenth of a bar, a quarter of a beat. |
| **Gate** | How long each step sounds, as a share of the step: 50% is short and choppy, 100% joins them. |
| **Note echo** | Each note played again, later and quieter, a few times. A delay, but of notes rather than sound. |
| **Kernel** | Sound code compiled to WebAssembly, which runs as the song plays ([kernel ABI](../../kernel-abi.md)). |
| **Event** | One message in the notes stream: a note starts (On), a note ends (Off), or a controller moves (the pedal, the wheels) ([RENDER-14](../../architecture.md#events)). |
| **MPE** | A way for a keyboard to bend or press each note of a chord on its own. Later (D10). |
| **Bake** | Write what a note effect plays into a clip as plain notes. |

## 1. What is missing, and why it matters

- A pop producer's synth part is often a held chord turned into a pattern: the arpeggio in a dance-pop chorus, a pluck that repeats each chord in sixteenths. Today you must draw every one of those notes by hand.
- A beginner can't play chords well. A Chord effect plays a whole chord from one key. A Scale effect makes every key you press fit the song's key, so no note is wrong.
- What music2 has already:

| Built | Where | What it gives |
|---|---|---|
| One event type for every source of notes | [RENDER-12, RENDER-14](../../architecture.md#events) | Clips, live keys, audition and the pedal all reach instruments as one stream of events, stamped to the sample. The type was made for event-producing kernels too ("live input, audition, code-generated patterns and event-producing kernels all plug into the same buffers"). |
| Kernels that hear notes | [ABI-8, ABI-9](../../architecture.md#kernel-abi-reservations) (track `events`) | A kernel may take a lane's notes each block. Nothing uses it yet. |
| Kernels that give notes | [ABI-2](../../architecture.md#kernel-abi-reservations), `docs/kernel-abi.md` "Reserved shapes" | **Reserved, not built.** The shape is fixed: the same header and 24-byte events as an input. The Deferred table waits for "the first arpeggiator". |
| Events made by code | [ABI-4](../../architecture.md#kernel-abi-reservations) | **Reserved, not built**: a `const-events` op in the IR. |
| Scales, the song's key | [NOTE-1 to NOTE-9](../../architecture.md#notes-and-scales), [VOX-10 to VOX-13](../../architecture.md#the-songs-key) | Thirteen scales in one table, chord stamps and grooves in the roll (edits), and `time.key`: the key as a signal a kernel reads, which changes with no rebuild. |
| `std/gen`, `std/theory` | [song-as-program.md](../../song-as-program.md) | **Designed, not built**: `euclid`, `arp`, `chords`, `scale`, `strum` as "generators that output lane data". |

How notes reach a sound today (from the code at `c653658c`):

| Step | Where | What happens |
|---|---|---|
| 1 | `NoteScheduler` (`core/runtime/src/sched.rs`) | Each block, before the graph runs, each notes lane fills its buffer from its clips: an On or Off at its exact frame. Live keys and audition join at the block's first frame ([MON-7](../../architecture.md#monitoring-overlays-and-mute-in-song)). Note ids are unique across the engine. |
| 2 | the checker (`core/ir/src/check.rs`) | A `poly`, a plugin's `notes` and a kernel's `events<note>` input must each read **a notes lane, straight** (or the note delay the latency planner put before one). Nothing else can feed notes. |
| 3 | `PolyNode::process` (`core/runtime/src/lib.rs`) | The voices take the notes: each On gets a voice, each voice reads its note's fields. |
| 4 | recording (`crates/server/src/graph/record.rs`) | A take records the keys as they were played, before any sound. |

So the gap is exactly one link: **nothing can give notes except a lane.**

## 2. What other DAWs and code-first tools do

Read on 2026-11-07. Each row names the page it comes from.

### 2.1 DAWs: note effects that play

| Product | What it has | What we learn | Source |
|---|---|---|---|
| **Ableton Live 12** | MIDI effects: Arpeggiator, CC Control, Chord, Note Length, Pitch, Random, Scale, Velocity. Note Echo is a Max for Live device. They chain, and MIDI Effect Racks run chains side by side. | "First MIDI effects, then an instrument, and finally audio effects": what is left of the instrument is notes. | [MIDI effect reference](https://www.ableton.com/en/live-manual/12/live-midi-effect-reference/), [instruments and effects](https://www.ableton.com/en/live-manual/12/working-with-instruments-and-effects/), [racks](https://www.ableton.com/en/live-manual/12/instrument-drum-and-effect-racks/), [Max for Live devices](https://www.ableton.com/en/live-manual/12/max-for-live-devices/) |
| | Arpeggiator: 18 styles (Up, Down, UpDown, Play Order, three kinds of Random…), rate synced or in ms, gate as a % of the rate (over 100% overlaps), steps that transpose, **Hold**, **Retrigger** Off / Note / Beat, velocity decay, groove. Chord: up to six added pitches, ±36 semitones, and a strum of up to 400 ms. Scale: a 12 × 12 map, or the clip's scale. | Many styles, but a beginner uses four or five. Retrigger and Hold are what decide when it starts. | [MIDI effect reference](https://www.ableton.com/en/live-manual/12/live-midi-effect-reference/), [Live 11's, with every style](https://www.ableton.com/en/live-manual/11/live-midi-effect-reference/) |
| | Scale awareness: the scale is set **per clip**, and Scale, Arpeggiator, Pitch, Random and Chord can follow it. | music2's key is the song's, one place ([VOX-10](../../architecture.md#the-songs-key)): simpler. | [Editing MIDI](https://www.ableton.com/en/live-manual/12/editing-midi/) |
| **Logic Pro** | MIDI FX: Arpeggiator, Chord Trigger, Modifier, Modulator, Note Repeater, Randomizer, Scripter, Transposer, Velocity Processor. The Arpeggiator plays Up, Down, Up/Down, Outside-in, As played…; rates 1/4 to 1/32, triplets, dotted; it stops when you let go unless Latch is on; one key held is repeated. | The same set as Ableton's, under other names: it is what a producer expects. | [MIDI plug-ins](https://help.apple.com/logicpro-effects/mac/10.2/en.lproj/lgcec21fdd03.html), [Arpeggiator](https://help.apple.com/logicpro-effects/mac/10.2/en.lproj/lgce129c3fbe.html), [Note Repeater](https://support.apple.com/guide/logicpro/note-repeater-controls-lgce9c809354/mac) |
| | **Scripter**: JavaScript, called per event (`HandleMIDI`) and once per block (`ProcessMIDI`). With timing on, each block knows its start and end beat and the tempo, and an event can be sent at a beat (`sendAtBeat`). | Code-written note effects are loved, and the model is **block by block, placed by beat**: what a music2 kernel does with `time.beats`. | [timing info](https://help.apple.com/logicpro/mac/10.4.5/en.lproj/lgcee186be46.html), [beat positions](https://help.apple.com/logicpro/mac/10.4.5/en.lproj/lgce8eb55106.html), [API summary](https://kabirnagral.github.io/LPXscripts) |
| **Bitwig Studio** | About 25 Note FX: Arpeggiator, Echo, Multi-note (a chord of up to 8), Key Filter, Latch, Note Length, Quantize, Strum, Harmonize… The Note Grid patches note modules by hand. Per-note expressions (pressure, timbre, micro-pitch) live in clips. | **Quantize can only make notes later**, never earlier, because it runs live; a "forgiveness" lets a slightly late note through. The same is true of an arpeggiator's first step (D7). | [Note FX](https://www.bitwig.com/userguide/latest/note_fx), [note events](https://www.bitwig.com/userguide/latest/working_with_note_events/), [Note Grid](https://cdm.link/2022/02/bitwig-4-2-note-grid-for-modular-note-processing-and-generation-plus-a-bunch-of-new-effects/) |
| **FL Studio** | An arpeggiator built into every channel. VFX note effects (Key Mapper, Keyboard Splitter, Sequencer) only inside Patcher. | A built-in arp per channel is easy to find; note effects hidden in a patcher are not. | [Channel settings](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/chansettings_misc.htm), [VFX Sequencer](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/VFX%20Sequencer.htm) |
| **Max for Live**, **Reaper JSFX** | A MIDI effect reads every event and must send on whatever should pass (`midiout`; `midisend` with a sample offset in the block). One that forgets drops it. | Passing the pedal and the wheels on is the effect's job; music2's kit should make it one line (§6.1). | [Max MIDI effects](https://docs.cycling74.com/legacy/max8/vignettes/live_midieffects), [JSFX MIDI](https://www.reaper.fm/sdk/js/midi.php) |

### 2.2 DAWs: tools that write notes

| Product | What it has | Source |
|---|---|---|
| **Ableton Live 12, MIDI Tools** | Transformations (Arpeggiate, Strum, Ornament, Chop…) and Generators (Rhythm, Seed, Stacks, Euclidean) in the clip view. They **replace the clip's notes**; they follow the clip's scale. Max for Live tools get the clip's notes as a list and give a list back. | [MIDI Tools](https://www.ableton.com/en/live-manual/12/midi-tools/), [Max MIDI Tools](https://docs.cycling74.com/max8/vignettes/live_miditools) |
| **FL Studio, piano roll scripts** | Python scripts that add, change and delete a clip's notes, with a dialog of controls and a preview. | [Piano roll scripting](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/pianoroll_scripting_api.htm) |

These are option A of §3: edits, done once. Ableton has both kinds, and keeps them apart.

### 2.3 Plugin standards: how a note travels

| Standard | What a note is | Source |
|---|---|---|
| **CLAP** | Events carry a sample offset in the block and come **sorted**; a plugin's output must be sorted too. A note has a `note_id` (or −1), a port, a channel, a key and a velocity 0 to 1. Note expressions (volume, pan, tuning, vibrato, expression, brightness, pressure) are addressed to one note. A note-effect plugin has a note input port and a note output port. | [events.h](https://raw.githubusercontent.com/free-audio/clap/main/include/clap/events.h), [note-ports.h](https://raw.githubusercontent.com/free-audio/clap/main/include/clap/ext/note-ports.h) |
| **VST3** | A note on has a pitch, a tuning in cents, a velocity, and a `noteId` that note expressions use to find their voice. | [NoteOnEvent](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/structSteinberg_1_1Vst_1_1NoteOnEvent.html), [note expressions](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/ivstnoteexpression_8h.html) |
| **MPE** | One sounding note per MIDI channel, so each has its own bend (±48 semitones by default), pressure and timbre (CC 74). | [Roger Linn Design](https://www.rogerlinndesign.com/support/support-developers-how-to-add-mpe), [MIDI.org](https://midi.org/community/midi-specifications/how-midi-mpe-pitch-bend-works) |

music2's `Event` ([RENDER-14](../../architecture.md#events)) already has the same parts: a frame in the block, an id, a pitch, a velocity, and room for per-note expression. A note effect needs no new note type.

### 2.4 Code-first music

| Tool | How notes are made | When | Source |
|---|---|---|---|
| **TidalCycles, Strudel** | A pattern is a **function of time**: asked for a span, it gives the events in it. A scheduler asks for the next slice each tick and plays it about 100 ms later. | ahead, by a little, any span in any order | [Tidal](https://tidalcycles.org/docs/innards/what_is_a_pattern), [Strudel](https://strudel.cc/technical-manual/patterns/), [its scheduler](https://codeberg.org/uzu/strudel/raw/branch/main/packages/core/cyclist.mjs) |
| **SuperCollider** | A pattern describes; a **stream** runs it, one step after another, with state. Sent 0.2 s ahead; live input uses no latency. | ahead, step by step from the start | [Patterns](https://doc.sccode.org/Tutorials/A-Practical-Guide/PG_01_Introduction.html), [Pbind](https://doc.sccode.org/Classes/Pbind.html), [timing](https://docs.supercollider.online/Guides/ServerTiming.html) |
| **Sonic Pi** | `live_loop`s run code in logical time, scheduled ahead; `chord(:E3, :minor)` is a list; random is seeded, so every run sounds the same. | ahead, step by step | [core.rb](https://github.com/sonic-pi-net/sonic-pi/blob/dev/app/server/ruby/lib/sonicpi/lang/core.rb), [randomness](https://github.com/sonic-pi-net/sonic-pi/blob/dev/app/web/web/data/tutorial/en/04-Randomisation.json) |

None of the three can follow a key played live without latency: they work ahead of the sound. A Strudel-style pattern that is a pure function of the beat fits a music2 kernel well: it reads `time.beats` and gives the notes that start in the block, the same live and in an export. So a pattern language can be TypeScript that turns a pattern into numbers at build, played by a kernel (§6.5).

### 2.5 What the research decides

| Finding | So music2 |
|---|---|
| Every DAW puts note effects before the instrument, in the same chain | does too (D2) |
| Arpeggiator, Chord, Scale, an echo of notes, Random, Velocity and Note length are the common set | starts with the first four (D3) |
| Ableton keeps "effects that play" and "tools that write" apart, and both are loved | builds the first now; Bake gives the second's result (D1) |
| Logic's Scripter places notes by beat, block by block | a kernel reads `time.beats` per block, to the sample |
| A live effect can only delay a note, never play it early (Bitwig's Quantize) | an arpeggiator on the grid waits for the next step (D7) |
| Max for Live and JSFX drop what an effect doesn't pass on | the kit passes controllers on in one line, and the guard makes sure no note hangs (D9) |
| Random is repeatable from a seed (Sonic Pi) | so is Random here (§6.1), so an export is the same every time |

## 3. Where notes are made: three options

| | **A. TypeScript at build** | **B. Note effects that play** | **C. Both, one note type** |
|---|---|---|---|
| What it is | Song or pack code makes notes once, when the song builds: patterns, generators, `std/gen` | A kernel takes notes in and gives notes out on the audio thread, each block, to the sample | B for anything that hears notes; A's generators written in TypeScript on top of B |
| Arpeggiator over a clip's chords | **No.** Build code may not read lane data ([RENDER-15](../../architecture.md#events)): a chord edit would need a rebuild, and agents' and the roll's edits would wait for one | Yes | Yes |
| Follows keys you play live | **No.** Build runs before you press a key | Yes | Yes |
| A generator (a Euclid rhythm, a pattern in code) | Yes | Yes: a kernel that gives notes without taking any | Yes, written in TypeScript |
| Bit for bit: export = playback ([RENDER-33](../../architecture.md#the-bit-identity-test)) | Yes | Yes: a kernel is a kernel, the same inputs give the same notes | Yes |
| Real time ([RENDER-28](../../architecture.md#plans-data-and-arrangement-swaps)) | Nothing on the audio thread | A small kernel, no memory taken after birth, time-limited as any pack's ([ABI-6](../../architecture.md#kernel-abi-reservations)) | as B |
| What the core gains | `const-events` in the IR, a scheduler for it, and a way to pass a lane's notes into build code (breaks RENDER-15) | **one** ABI shape, already reserved; see §5 | as B: nothing more |
| A beginner adds it from + | Only as an edit ("Arpeggiate this clip"), not a sound | Yes: + Note effect, before the instrument | Yes |
| What an agent sees | the notes, after the build | the code (`arp(lane.notes("chords"), { rate: "1/16" })`), the card, and what it plays through Bake's preview | both |
| Ableton's MIDI effects it makes possible as packs | none of them; only Live 12's clip **transformations** and **generators** (they write notes into clips) | all of them: Arpeggiator, Chord, Scale, Random, Velocity, Note Length, Pitch, and Note Echo (a Max for Live device there). CC Control needs a kernel that gives controllers, which the same shape allows later | all of them, and the generators |

A is good for one thing, generators, but can't be the answer to the arpeggiator, the chord tool or anything that follows the keys. A clip tool that rewrites notes ("Arpeggiate this clip", as Live 12's transformations) is an edit, and music2 already has the edit path for it (chord stamps, grooves, [NOTE-4](../../architecture.md#notes-and-scales), [NOTE-7](../../architecture.md#notes-and-scales)); it is another feature, not this one.

**Recommendation: C, at B's cost.** Build B's one primitive. Generators are then TypeScript that works out a pattern at build (numbers) and hands it to a pack's kernel that plays it from the song's beat. That is what C asks for, with no second mechanism in the core. ABI-4's `const-events` stays unbuilt (D11).

## 4. How it feels

### 4.1 On the track

```
keys you hold ─▶ [Arpeggiator] ─▶ [Chord] ─▶ [Pluck] ─▶ [Reverb] ─▶ mixer
                 note effects                instrument   effects
```

- **Note effects sit before the instrument**, left of it in Device, in the order the notes pass through them. A small "notes" arrow joins them; the instrument's card is where notes become sound.
- **+ Note effect** sits before the first card, as **+ Effect** sits after the last. It lists note effects only (a component that takes notes first and gives notes, §5.4).
- ⋯ on a note effect's card has what an effect's has: **Bypass** (the notes pass through it unchanged), **Move left / right**, **Remove**, and one more: **Bake to notes** (§4.4).
- Each card is the cards2 frame ([cards2](../cards2/design.md) §3): a face with its picture and its few knobs, a tab or two, and All.

![The Arpeggiator's card in the mockup](images/mockup.png)

### 4.2 In the song's code

A note effect is a function from notes to notes, so the code reads in the order the notes go, inside out, as effects do:

```ts
import { track } from "@m2/core";
import { pluck } from "@m2/synths";
import { arp, chord } from "@m2/notes";

// Hold a chord (or draw one in "chords"): its notes play one at a time, a sixteenth each, up two octaves.
export default track("keys", { name: "Keys" }, ({ lane }) =>
  pluck(arp(lane.notes("chords"), { rate: "1/16", octaves: 2 })),
);
```

```ts
// One finger plays a whole chord of the key; Scale first, so a wrong key can't happen.
pad(chord(scale(lane.notes("keys")), { shape: "7th" }))
```

```ts
// One arpeggio, two sounds: notes are a value, so they can feed two instruments.
const notes = arp(lane.notes("chords"), { style: "updown" });
return sum(pluck(notes), synth.pad(notes));
```

```ts
// A pack's generator (later, §7 track 5): TypeScript works out the steps at build; the kernel plays them.
drums.drum(euclid({ hits: 5, steps: 16 }), { drum: "rim" })
```

When you add one with **+ Note effect**, the app writes the same code: it wraps the instrument's notes, `pluck(lane.notes("chords"))` becomes `pluck(arp(lane.notes("chords")))`, one undo step. Nothing about it is hidden state.

### 4.3 Playing it live

- Hold keys on a MIDI keyboard, or the computer's keys, on the track: the arpeggiator plays them. Let go and it stops, unless **Hold** is on.
- **While the song plays**, steps land on the song's grid (`time.beats`), so the arpeggio is in time with the drums.
- **While the song is stopped**, the first step plays as you press the key, and the next ones follow at the song's tempo (D4).
- A key clicked in the roll (audition) plays through it too: audition joins the lane as live keys do.
- It adds no delay: a key's notes come out in the block it goes in. (Live keys join a lane at the next block's first frame, about 2.7 ms, as they do today.)

### 4.4 Keeping what it played

| You want | You do | What happens |
|---|---|---|
| Record a part with the arp on | **Record**, as now | The take holds **the keys you pressed**, not the arp's notes (as Ableton and Logic do). Playing it back, the arp plays them again, so you can still change its rate. |
| Keep the arpeggio itself, to edit its notes | ⋯ ▸ **Bake to notes** on the arp's card | music2 renders the lane's clips, offline from the song's start, through the note effects up to and including that card, and writes what came out as clips on the same lane, in place of the clips it baked; the note effects baked are bypassed (D5), so you hear the same notes. Live keys are never baked: only what the clips play. One Tx, one undo step. |
| An agent wants to know what it plays | `m2 call` the bake route with `preview: true` | The notes, as JSON, written nowhere. |

## 5. What the core gains, counted

| # | Layer | Gains | Count |
|---|---|---|---|
| 1 | Kernel ABI | **A kernel may give notes**: an output of kind `events<note>`, in ABI-2's reserved shape (§5.1). Keeps v1. | **1 reserved shape built** |
| 2 | IR and the checker | **Any notes input may read a note effect**: a `poly`, a plugin's `notes` and a kernel's `events<note>` input read a notes lane *or* a kernel's notes output, at a module's top level (§5.2) | **1 rule widened** (the three "straight from a lane" refusals) |
| 3 | Runtime | **The note guard** on each notes output: checks, orders and numbers what a kernel gives, and ends every note it started (§5.3) | **1 part** |
| 4 | SDK (`@m2/core`) | none. `kernel<EventsKind>(…)` already types a kernel's output as notes, and `port.notes()` as a component's output is already a type; the classifier learns one more shape: notes first and notes out is a **note effect** (§5.4) | **0 words, 1 rule in the classifier** |
| 5 | Time | **`time.tempo`**, the song's tempo where it is, in beats per minute, song time ([RENDER-7](../../architecture.md#live-engine-and-its-two-clocks)) like `time.beats`. Only if D4 is yes. | **1 signal, 1 unit** |
| 6 | Editor and routes | + Note effect's insert (wrap an instrument's notes argument), bypass of a note effect; **Bake to notes**: 1 route and 1 `m2` command, `m2 bake` | **1 write kind, 1 route, 1 command** |
| 7 | Window | note effects left of the instrument in Device, + Note effect | **1 change to the chain** |
| 8 | std | nothing. No new std kernel. | **0** |

Everything else is in a pack: `@m2/notes`, with its own kernels (`packs/@m2/notes/kernels-src`, built by `std/build.sh`, [ABI-10](../../architecture.md#kernel-abi-reservations)).

### 5.1 A kernel may give notes

- A kernel's manifest may list an output of kind `events<note>`: `outputs = ["out: events<note>"]`. Its one entry in the port table is the address of a header `{count: u32, cap: u32, events: address}`, then `cap` events of 24 bytes, RENDER-14's `Event` as `docs/kernel-abi.md` lays it out. The host places them at birth, in the state's own block, and sets `cap`. Before each call it sets `count` to 0; the kernel writes its events and sets `count`.
- **The room is fixed: 1,024 events a block.** That is eight for every frame, far more than any music. A kernel that writes more is cut there (the guard says so once, §5.3).
- Like a kernel that takes notes, a kernel that gives notes runs at a module's top level on whole blocks (`from` 0, `to` 128), so an event's frame is also its place in the call's audio and control inputs. Inside a `poly` voice it is refused in words: a voice makes sound, not notes.
- A kernel that gives notes may take notes (an arpeggiator), or not (a generator that reads `time.beats`), or take audio (a drum trigger that hears the kick and gives a note on each hit). One primitive serves all three.
- The Rust kit gets `Io::give(i)`: a writer for output `i` that takes `Event`s in order and can't write past `cap`, as `Io::events(i)` reads ([ABI-9](../../architecture.md#kernel-abi-reservations)).
- **Why it keeps kernel ABI v1** ([ABI-3](../../architecture.md#kernel-abi-reservations)): the shape was fixed among v1's reservations, and a module with no notes output sees nothing new.

### 5.2 Any notes input may read a note effect

- Today the checker refuses notes from anything but a lane, in three places (a `poly`'s, a plugin's, a kernel's). Each becomes: **a notes lane, or a kernel's notes output, at a module's top level**, through a note delay where the latency planner puts one ([RENDER-18](../../architecture.md#latency)). Chains (arp → chord → scale → pluck) run in graph order, as audio does.
- A note source's shape joins its consumers' shape ([RENDER-25](../../architecture.md#plans-data-and-arrangement-swaps)), as a lane's does today ([ABI-8](../../architecture.md#kernel-abi-reservations)): an instrument rewired from a lane to an arpeggiator is kept, and first ends every note it was playing ([RENDER-26a](../../architecture.md#plans-data-and-arrangement-swaps)).
- The voices' rules all hold on a note effect's notes: the pedal ([RENDER-38](../../architecture.md#voices)), a key's count ([RENDER-39](../../architecture.md#voices)), mono and legato ([RENDER-16](../../architecture.md#voices)). A plugin gets them as CLAP note events, as it gets a lane's.

### 5.3 The note guard

A pack's kernel is a stranger's code. A note it starts and never ends would hang until the song closes. So the host reads every notes output through a guard, which allocates nothing and holds a fixed room per output:

| The kernel gives | The guard |
|---|---|
| events out of order | puts them in RENDER-14's order (by frame; at one frame controllers, Offs, Ons), stably, so the same input gives the same order |
| a frame outside the block, a kind it doesn't know, a pitch outside 0 to 127 or not a number, a velocity outside 0 to 1 | drops the event |
| an On | gives it a fresh engine-wide note id ([RENDER-26a](../../architecture.md#plans-data-and-arrangement-swaps)), keeping the kernel's own id beside it; an On whose id is already sounding first ends the old one |
| an Off | sends it only for a note sounding from this kernel; drops it otherwise |
| more notes sounding than its room (256) | drops the new On, so its Off is dropped too |
| more than 1,024 events | keeps the first 1,024 |

And it ends every note the kernel has sounding, at that frame, when:
- the song stops or jumps (a seek, a loop's wrap): as a lane does ([RENDER-9](../../architecture.md#seek-and-loop)); a note effect still gives new notes after it, so a live arpeggio plays on while stopped;
- the kernel is reset (a panic, Stop All Sound) or replaced by a new plan;
- the kernel is stopped for taking too long ([ABI-7](../../architecture.md#kernel-abi-reservations)).

A guard that drops something says so once per plan, naming the sound and its pack, as `sound.too-slow` does ("Arp on Keys, from @alice/arp, gave notes music2 can't play; some were left out."). So **no note effect can leave a note hanging**, and every instrument after one hears only notes that start and end.

### 5.4 A note effect is a component

No new descriptor field: the types say what it is ([cards](../cards/design.md) §2, "one declaration").

| A component's first input | Its output | The editor calls it | Offered by |
|---|---|---|---|
| notes | audio | an instrument (today) | + Track, Change instrument |
| audio | audio | an effect (today) | + Effect |
| **notes** | **notes** | **a note effect** (new) | **+ Note effect** |

Its `editor.category` groups it in that list (Arpeggiator, Chords, Scales, Generators…), as categories group effects ([PKG-7](../../architecture.md#component-descriptors)).

### 5.5 Rules that change

The contract changes first, in each track (AGENTS.md). The rules this touches:

| Rule | Today | With note effects |
|---|---|---|
| [ABI-2](../../architecture.md#kernel-abi-reservations) | event outputs reserved | built (§5.1); `k_save`, `k_latency` stay reserved |
| [ABI-4](../../architecture.md#kernel-abi-reservations) | `const-events` reserved | dropped, if D11 is yes |
| [ABI-8](../../architecture.md#kernel-abi-reservations) | a kernel's notes come straight from a lane | from a lane or a note effect (§5.2) |
| [RENDER-12](../../architecture.md#events) | a notes lane fills each buffer | a lane or a note effect's guard |
| [RENDER-26a](../../architecture.md#plans-data-and-arrangement-swaps) | a scheduler owns its sounding notes | a note effect's guard owns its own the same way (§5.3) |
| [RENDER-7](../../architecture.md#live-engine-and-its-two-clocks) | `time` reads beats, seconds, playing, key | and tempo, if D4 is yes |
| [PKG-3](../../architecture.md#component-descriptors) | `bypass` on an audio-to-audio call | and on a notes-to-notes call |
| [PKG-7](../../architecture.md#component-descriptors) | notes first is an instrument | notes first and notes out is a note effect (§5.4) |
| [KIT-1](../../architecture.md#drum-kits) | a Drum fed by anything but its own lane is in no kit | a Drum fed by its own lane **through note effects** stays in its kit, so note repeat works on a kit's hats |
| [NOTE-9](../../architecture.md#notes-and-scales) | "Not yet: an arpeggiator" | gone; the guide's [music basics](../../guide/music-basics.md) too |

New rule ids are given out by the lead.

### 5.6 Why TypeScript alone can't

- **Build code can't see notes.** A song's TypeScript runs once, when the song builds, and may not read the lanes ([RENDER-15](../../architecture.md#events)), so a note drawn in the roll is heard without a rebuild. An arpeggiator must read the notes.
- **Build code can't see keys played live.** It has finished before you touch the keyboard.
- **Nothing that exists turns signals into notes.** A `poly` turns notes into sound; a kernel turns signals into signals. The only source of notes is a lane. A note effect needs a node whose output is notes, and that is the one thing §5.1 adds.
- **Running TypeScript on the audio thread is no answer.** Logic's Scripter runs JavaScript there, but music2 runs pack code in a build sandbox with a garbage collector; the audio thread must never wait ([RENDER-28](../../architecture.md#plans-data-and-arrangement-swaps)). Kernels are how music2 runs code as the song plays, and their language doesn't matter.

What stays TypeScript: every note effect's card, its choices and its words; generators' patterns, worked out at build and handed to a kernel as numbers (`values`, [ABI-2](../../architecture.md#kernel-abi-reservations)); `std/theory`-style helpers in a pack.

## 6. The first pack: `@m2/notes`

Four note effects, each a component in TypeScript over one kernel of the pack's own. Each choice is a control (a menu that changes while the song plays, with no rebuild, [CARD-10](../../architecture.md#kinds-of-values)), so a lane can automate it: an arpeggio that goes from 1/8 to 1/16 in the build.

### 6.1 Arpeggiator

```ts
export const arp = component(
  {
    id: "arp", label: "Arpeggiator",
    inputs: { notes: port.notes() },
    controls: {
      style: control.choice(["up", "down", "updown", "played", "random"], "up"),
      rate: control.choice(["1/4", "1/8", "1/16", "1/32"], "1/16"),
      triplet: control.bool(false),
      octaves: control.choice(["1", "2", "3", "4"], "1"),
      gate: control.scalar(0.8).range(0.1, 2),
      hold: control.bool(false),
      velocity: control.choice(["played", "fixed"], "played"),
      swing: control.frac(0.5).range(0.5, 0.71),
    },
    settings: { seed: setting.int(1) },
    output: port.notes(),
    editor: {
      category: "Arpeggiator",
      views: { steps: stepsPicture() }, // the steps, lit by the kernel's `step` meter
      face: ["steps", "style", "rate", "octaves", "gate", "hold"],
      groups: { Feel: ["velocity", "swing", "triplet"] },
    },
  },
  // `time.tempo` only if D4 is yes; without it the arpeggiator steps only while the song plays.
  (notes: Events, { style, rate, triplet, octaves, gate, hold, velocity, swing, seed }: Params) =>
    kernel<EventsKind>(
      "@m2/notes:arp",
      { notes, beats: time.beats, playing: time.playing, tempo: time.tempo, style, rate, triplet, octaves, gate, hold, velocity, swing },
      { values: [seed] },
    ),
);
```

| It does | Exactly |
|---|---|
| Which notes it walks | the notes held now, in the order of `style`: by pitch (up, down, up and down, the top and bottom played once each turn), as they started ("As played"; notes that start on one frame by pitch), or at random; repeated `octaves` times, an octave higher each time |
| When a step plays | while the song plays: at each multiple of the rate on `time.beats` (a sixteenth is a quarter of a beat), so it is on the grid; while stopped: the first step as the first key goes down, then one each rate at `time.tempo` |
| How long | `gate` × the step: under 1 is choppy, over 1 overlaps the next step |
| How loud | the velocity of the note it plays, or a fixed 0.8 |
| Swing | the second step of each pair later, as the roll's swing ([NOTE-7](../../architecture.md#notes-and-scales)): 50% is straight, 66% a shuffle |
| Hold | the last chord stays held after you let go, until you press a new one |
| Random | from `seed`, started again at each reset: the same song renders the same arpeggio every time ([RENDER-33](../../architecture.md#the-bit-identity-test)) |
| Controllers | passed on as they come: the wheels and the pedal reach the instrument |
| One key held | repeated at the rate: on a Drum track that is **note repeat**, the hi-hat roll of trap and pop (as Logic's arpeggiator repeats one key) |

The card: its face is the **steps picture**: the arpeggio drawn as short bars, low to high, for the notes held (or a C major chord while none are), the step playing lit by a meter ([ABI-13](../../architecture.md#kernel-abi-reservations)); its words say it, "Up, sixteenth notes, 2 octaves: C4 E4 G4". The face's knobs are Style (a segmented control: five options), Rate, Octaves, Gate and Hold. **Feel** holds velocity, swing and triplets. Then All.

### 6.2 Chord

`chord(notes, { shape: "triad", inKey: true, inversion: 0 })`: each note you play becomes a chord.

- **Shape**: Octave, Fifth, Triad, 7th, sus2, sus4, add9.
- **In key** (on by default): the chord is stacked from the song's key, from `time.key` ([VOX-12](../../architecture.md#the-songs-key)), the same stacking as the roll's chord stamps ([NOTE-4](../../architecture.md#notes-and-scales)), so one finger on D in C major plays D minor. A key pressed off the key is moved to its nearest note first, a tie going down ([NOTE-3](../../architecture.md#notes-and-scales)). With no key, or **In key** off, the shapes are fixed (a triad is major). A test checks the kernel's chords against the stamps table for every key, so the two never drift.
- **Inversion**: 0 to 3.
- The card's face: a small keyboard with the notes added to a C drawn, and the chord's name ("C → C E G, a triad").

### 6.3 Note echo

`echo(notes, { time: "1/8", repeats: 3, fall: 0.3, pitch: 0 })`: each note played again `repeats` times, `time` apart (in beats, so it follows the tempo), each `fall` quieter, and `pitch` semitones higher (+12 climbs by octaves). Stopped, it keeps time by `time.tempo`, as the arpeggiator does. The face: the echoes drawn as fading bars.

### 6.4 Scale

`scale(notes, { shift: 0 })`: every note moved to the nearest note of the song's key, a tie going down, as Lock to key does in the roll. `shift` moves each note up or down that many notes **of the key** (+2 in C major turns C E G into E G B). With no key, it says "Set the key" on its card, and plays notes as they are ([VOX-12](../../architecture.md#the-songs-key)). The face: the twelve notes, the key's lit.

### 6.5 Later, as the pack's minors (not in the first track)

| Note effect | Does | Ableton's |
|---|---|---|
| Random | moves some notes by chance, in the key | Random |
| Velocity | makes notes louder, softer, or more alike | Velocity |
| Note length | makes every note a set length | Note Length |
| Strum | spreads a chord's notes in time, low to high | Live 12's Strum transformation |
| Euclid, Pattern | generators: a rhythm or a pattern written in TypeScript, played from the beat | Live 12's generators, Strudel's patterns |

## 7. Build order

Each track has a builder, a skeptic who attacks what it built, and the builder again, as in [Typed sounds](../cards/design.md) §11. Rule ids are given out by the lead; the contract changes first in each track.

| # | Track | Builds | Size |
|---|---|---|---|
| 1 | `notes-out` | §5.1 to §5.4: notes outputs in `KernelSig` and the sandbox; the kit's `Io::give`; the checker's notes sources; the guard; shapes and activations; the classifier's note effect; `time.tempo` (if D4). Tests with hand-written kernels in `core/runtime/src/tests/events.rs`: a kernel's notes reach a `poly` and a plugin to the frame; a chain of two; a kernel that hangs a note, writes out of order, writes too much or is stopped never leaves one sounding; live = offline bit for bit | medium to large (about track `events`) |
| 2 | `notes-pack` | `@m2/notes` 0.1.0: the four kernels (`packs/@m2/notes/kernels-src`) with the kit's tests, the components, an example song (`examples/arp`) recorded in `renders.json`, the guide's page | medium |
| 3 | `notes-chain` | the window and the writes: note effects left of the instrument in Device, + Note effect, wrap and unwrap, bypass and move; the four cards in the cards2 frame, their pictures and the arp's step meter; `e2e` suites | medium |
| 4 | `bake` | Bake to notes: the route (with `preview`), `m2 bake`, the card's menu; an offline render of the track that reads the last note effect's guard, block by block, turns each frame into a beat by the tempo map, and writes the notes as a clip, one Tx; the guide | small to medium |
| 5 | `gen` (later) | generators as a pack (`@m2/gen` or `@m2/notes` 0.2): Euclid and a pattern language in TypeScript over one "play these steps" kernel; Random, Velocity, Note length, Strum | medium |

Tracks 1 and the pack's kernel code (2) can start together against the reserved shape; 3 and 4 need 1 and 2.

## 8. Not now

| What | Why later |
|---|---|
| Per-note bends and MPE (each note of a chord bent on its own) | RENDER-14 reserves `NoteExpression`, and a note effect can pass them on once keyboards send them. Recording them needs a format change ([gap #24](../../gaps.md)). D10. |
| CLAP plugins that are note effects (a plugin arpeggiator) | music2 reads a plugin's notes out only for parameter gestures today; a plugin note effect would be §5.2's notes source made by a plugin. Wait for a plugin people ask for. D12. |
| A note effect on another track's notes (one arp driving two tracks) | Notes are a value inside a track already (§4.2's third example). Across tracks needs a track to export notes, a separate design. |
| A key that changes part way through the song | gap #16's own "Left"; Scale and Chord follow `time.key` and will follow a changing key with no change of theirs. |
| Clip tools ("Arpeggiate this clip" as an edit) | An edit, not a sound; Bake gives the same result from a note effect. |

## 9. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | Should arpeggiators, chord tools and the like be **sounds that play** (they follow the keys you hold, and you can change them any time), rather than **tools that write notes** into clips once? | **Sounds that play**, with **Bake to notes** to write them into a clip when you want to edit them. It is how Ableton, Logic and Bitwig do it, and the only way an arpeggiator follows a keyboard. |
| D2 | Should a note effect sit **before the instrument on the same track**, shown left of it in Device, with its own **+ Note effect** button? | **Yes.** Notes go left to right, then sound does. Every DAW puts them there. |
| D3 | The first four: **Arpeggiator, Chord, Note echo, Scale**, in a new pack, `@m2/notes`. Right four, right name? | **Yes.** Random, Velocity, Note length and Strum follow as the pack grows. |
| D4 | Should the arpeggiator **keep playing while the song is stopped** and you hold keys? It needs the song's tempo as a signal (one small addition). | **Yes.** You try ideas on the keyboard before you press Play. |
| D5 | **Bake to notes**: after it writes the clip, should the note effect be **turned off** (bypassed, still there) or **removed**? | **Turned off.** The clip plays what you heard; turn it back on to hear it twice, or remove it. One undo step either way. |
| D6 | When you **record** with an arpeggiator on, should the take keep **the keys you pressed** or **the notes the arpeggiator played**? | **The keys you pressed.** You can change the arpeggio afterwards; Bake keeps its notes when you want them. Ableton and Logic do the same. |
| D7 | While the song plays, should the arpeggiator's steps always **land on the song's grid**, even if you press a key a little late? | **Yes.** It stays in time with the drums. While stopped, it starts as you press (D4). |
| D8 | Should **Chord** build its chords **from the song's key** (one finger on D in C major plays D minor), as the roll's chord stamps do? | **Yes, by default**, with a switch for fixed shapes. A beginner can't play a wrong chord. |
| D9 | Should a pack's note effect be **guarded**, so that one that misbehaves can never leave a note hanging, and is named when it does? | **Yes.** It is the same promise as the time limit: a stranger's code can't break your song. |
| D10 | **Bending each note of a chord on its own** (MPE): now or later? | **Later** (gap #24). Note effects pass the wheels and the pedal on from the start. |
| D11 | ABI-4 reserved a way for song code to put notes straight into the engine. Generators can be a pack's kernel fed numbers instead. **Drop that reservation**, so there is one way notes are made? | **Yes, drop it.** One mechanism, not two. |
| D12 | **CLAP plugins that are note effects** (a plugin arpeggiator): now or when someone asks? | **When someone asks.** music2's own four cover the common needs. |
