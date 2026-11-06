# Vocals: tune a voice, edit its notes, comp the best take

| | |
|---|---|
| **Status** | Proposed, for the owner's yes (AGENTS.md rule 11). Nothing here is built. Rules: VOX-1 to VOX-n in the contract, written by the tracks of §8. |
| **Written** | 2026-11-06, from `main` at 8d37e209 |
| **Serves** | gap 50 (pitch correction), gap 51 (editing a vocal's notes), gap #25 (comping). It also gives gap 52 (slicing), gap 54 (vocal sounds) and gap #16 (the key in code) the parts they need |

**In short**
- **Tune** is a new effect for a voice. It moves each sung note onto the nearest note of the song's key. One knob, **Speed**,
  goes from "robot" (the hard, stepped sound of pop vocals) to "natural". It works on a voice while you sing and on a
  recorded clip, and an export sounds the same as playback.
- **A voice's notes** can be edited like the piano roll's. **Find notes** turns a recorded vocal into notes you can
  see. You drag a note up to fix its pitch, or sideways to fix its timing. Each edit is saved in the clip's own data,
  so the song's files say what plays.
- **Comping** builds one take from the best parts of several. You record a line a few times round a loop. Then you
  swipe across the take you like for each part. It is one undo step, and it saves only plain audio clips: nothing
  new in the song's format.
- **Classic sound maths only**, nothing learned from data (§7). Pitch is found by YIN, a formula from 2002. It is
  moved by PSOLA, which keeps the voice's own character (its formants).
- **The core gains little** (§6): no new kernel in std; one std kernel learns a second way to render; two fields in the
  song's files; one time signal; and two general tools that other features want too. Everything else is a new pack,
  `@m2/vocals`.

## Words used here

| Word | Meaning |
|---|---|
| **Pitch** | How high or low a sound is. A sung note has one; a "s" or "t" has none. |
| **Semitone** | The step from one piano key to the next. 100 **cents** make a semitone. |
| **Key** | The song's set of notes that sound right together, such as A minor ([NOTE-1](../../architecture.md#notes-and-scales)). |
| **Formants** | The "vowel" colour of a voice. If they move with the pitch, a voice sounds like a chipmunk or a giant. |
| **Latency** | How late a sound comes out of an effect. Music2 delays the other tracks to match ([RENDER-18](../../architecture.md#latency)). |
| **Take**, **pass** | One recording of a part. Recording round a loop makes one pass per loop, all in one file ([MON-11](../../architecture.md#monitoring-overlays-and-mute-in-song)). |
| **Comp** | One take made from the best parts of several. |
| **Kernel** | A small program, in WebAssembly, that works on sound sample by sample. TypeScript connects kernels but can't do that work itself. |

## 1. What a pop producer needs

| Need | What they use today | In music2 |
|---|---|---|
| A voice locked to the key, softly or as a hard effect | Auto-Tune, Logic's Pitch Correction | **Tune**, an effect (§3) |
| One wrong note fixed, a line moved in time | Melodyne, Logic's Flex Pitch | **Notes** of an audio clip (§4) |
| The best line from five takes | Logic's take folders, Ableton's take lanes | **Comp** across a loop's passes (§5) |

The big tools (Auto-Tune, Melodyne) aren't CLAP plugins, as far as is known, so music2 can't host them (gap #33). So
these are music2's own.

## 2. How open tools do it

| Tool | Finds the pitch by | Moves it by | What we take | Licence |
|---|---|---|---|---|
| [zita-at1](https://kokkinizita.linuxaudio.org/linuxaudio/zita-at1-doc/quickguide.html), and [x42's fat1](https://github.com/x42/fat1.lv2) built on it (`src/retuner.cc`) | Autocorrelation through an FFT, every 4 blocks of 128; 60 to 1,200 Hz; "voiced" only when it matches above 0.8 | Reading the input faster or slower, jumping by a whole pitch period with a short crossfade when it drifts too far; about 27 ms late (half in its low-latency mode) | The nearest allowed note, a **bias** that keeps the current note, a correction that jumps when the note changes and is smoothed while it stays, and an **amount**. Its pitch is about 10 ms older than the sound it corrects | GPL-2 |
| Autotalent (Tom Baran; [Debian's package](https://packages.debian.org/sid/autotalent)) | Autocorrelation | Overlap-add of short grains | Allowed notes, strength, and **formant correction**: the voice's colour put back after the shift | GPL |
| [Rubber Band](https://breakfastquay.com/rubberband/technical.html) | (none: it doesn't track pitch) | A phase vocoder, pitch as a resample plus a stretch the other way | That a stretch and a pitch shift are one job. music2's `std:stretch` already does it for loops (WSOLA) | GPL or paid |
| [Praat](https://www.fon.hum.uva.nl/praat/manual/Manipulation.html)'s Manipulation | Its pitch analysis, then one mark per vocal pulse | Overlap-add from the sound, its pulses, a **pitch tier** and a **duration tier** | The model for editing notes: the sound stays, and the edits are two small lists of what to change, rendered from them | GPL |
| [WORLD](https://github.com/mmorise/World) (Morise) | Harvest or DIO | Splits a voice into pitch, colour and noise, then builds it again | A better method for big moves, if PSOLA isn't enough (D7) | Modified BSD |
| [pYIN](https://code.soundsoftware.ac.uk/projects/pyin) and Tony (Mauch and Dixon, 2014; Mauch et al., 2015) | YIN with several candidates per frame, smoothed by a hidden Markov model | (none) | How to turn a pitch line into notes: pYIN's smoothing and Tony's note finder are both hand-set models, not trained ones | GPL |

The methods are published: YIN (de Cheveigné and Kawahara, *JASA* 111(4), 2002) and TD-PSOLA (Moulines and
Charpentier, *Speech Communication* 9, 1990). music2 is MIT or Apache-2.0, so it copies no GPL code. Each kernel is
written from the papers. Auto-Tune's first patent ([US 5,973,252](https://patents.google.com/patent/US5973252), filed
1998) is listed as expired. Melodyne's chords-in-a-mix method (DNA) is patented, and music2 doesn't try it (D9).

## 3. Tune: pitch correction as an effect

**Using it**

| You do | What happens |
|---|---|
| + Effect ▸ **Tune** on the Vox track | `tune(…)` is written into the track's code, as any effect is. The voice snaps to the song's key |
| Speed to 0 | The robot sound: each note jumps to its pitch at once |
| Speed to about 0.2 s | A gentle fix: the note slides to its pitch, and vibrato stays |
| Amount to 50% | Half of each note's error is fixed |
| Sing with Hear yourself on ([MON-8](../../architecture.md#monitoring-overlays-and-mute-in-song)) | You hear yourself tuned, about 15 ms late. The take records your dry voice |
| Export | The same sound as playback, bit for bit ([RENDER-1](../../architecture.md#offline), [RENDER-2](../../architecture.md#offline)) |

The card has two knobs, **Speed** and **Amount**. The key is the song's key. With no key set, every note is allowed.

**How it works.** Three kernels in a row, joined in TypeScript in `@m2/vocals` (a sketch, not final code):

```ts
export const tune = component({ id: "tune", label: "Tune", /* … */
    controls: { speed: control.secs(0.05).range(0, 0.4), amount: control.frac(1) } },
  (x, { speed, amount }) => {
    const sung = kernel("@m2/vocals:pitch", { in: x });                    // its pitch, and how sure
    const by = kernel("@m2/vocals:snap", { ...sung, key: time.key, speed, amount }); // semitones to move
    return kernel("@m2/vocals:shift", { in: x, semitones: by, hz: sung.hz }); // moved, formants kept
  });
```

| Kernel | What it does | Method |
|---|---|---|
| `pitch` | Finds the voice's pitch (`hz`), how sure it is (`clarity`, 0 to 1) and its loudness, every 256 samples | YIN: the difference between the sound and itself one period later, found by an FFT as zita-at1 does, then a parabola for the exact peak. 70 to 1,200 Hz. It looks only at the past, so it adds no latency |
| `snap` | Works out how many semitones to move: from the sung pitch to the nearest note of the key, a bias of 0.2 semitones keeping the current note, eased at `speed`, times `amount`. Where `clarity` is low (an "s", a breath), it eases back to 0 | Plain maths with a little memory. It is the only part made just for Tune |
| `shift` | Moves the pitch by `semitones` and keeps the formants. A `formant` input moves the colour on its own (0 for Tune) | TD-PSOLA: one grain two periods long around each vocal pulse, laid out again at the new spacing. A grain keeps its shape, so the vowel stays. Sounds with no pitch pass through. It looks one period ahead, so it is late by about 15 ms (720 samples at 48 kHz) |

**Why kernels, not TypeScript.** TypeScript in music2 builds the graph once; it never sees a sample. Finding a
period, keeping a note in memory and laying out grains are all work done on samples. std's math kernels (`add`,
`scale`, `mtof`…) have no logarithm, no memory and no grains, so the chain can't be built from them.

**Why three kernels, not one.** Each is useful alone. `pitch` serves a tuner, a voice that plays a synth, and slicing
by note (gap 52). `shift` serves a harmony, an octave effect and formant shifting (gap 54). Only `snap` is Tune's own.
They start as `@m2/vocals`' own kernels. Each moves to std once a second pack plays it, as std's design says
([cards §7.1](../cards/design.md)). Their shared parts live in `m2-dsp`, so `std:stretch` (§4) uses the same code.

**The song's key, read by sound.** Today the key lives in `layout.json`, which code never reads (gap #16,
[FMT-19](../../architecture.md#presentation-and-local-files)). But a key is part of the music, like tempo. So it moves
into `data/arrangement.json` beside the tempo, as `key: [{beat, root, scale}]`, and sound code reads it as
**`time.key`**: a signal, like `time.playing`. Its value holds the key's notes as twelve on/off bits. A key change at a
beat takes effect there. It is a signal, not something the build reads, so changing the key rebuilds nothing
([RENDER-15](../../architecture.md#events)). `m2 upgrade` moves an old song's key (D2).

**Latency.** A kernel's latency is a fixed number in its manifest today. `shift`'s depends on the sample rate, so
this builds the `k_latency` call that kernel ABI v1 already reserves ([ABI-2](../../architecture.md#kernel-abi-reservations);
its trigger, "a kernel with latency", arrives). No new ABI version. The other tracks are delayed to match
([RENDER-18](../../architecture.md#latency)); the delay also means `pitch` sees about 15 ms ahead of the sound `shift`
moves, so the correction lands on time.

## 4. A voice's notes: edit pitch and timing

**Using it**

| You do | What happens |
|---|---|
| Right-click a vocal clip ▸ **Find notes** | music2 finds each sung note, then opens the clip in the Notes view. One undo step |
| Look | The piano roll, with the clip's waveform faint behind. Each sung note is a bar at its pitch, as thick as it is loud, with the sung pitch line drawn through it. Notes outside the key are shaded ([NOTE-2](../../architecture.md#notes-and-scales)) |
| Drag a note up or down | It moves by whole semitones; ⌘ moves it by cents ([NOTE-6](../../architecture.md#notes-and-scales)); with Lock to key, onto the key ([NOTE-3](../../architecture.md#notes-and-scales)) |
| Drag a note sideways, or its edge | Its timing moves (warp markers at its edges, D6) |
| Double-click a note | It moves to the nearest note |
| **Straighten** (a slider in the note's menu) | Its wobble and vibrato are flattened, from 0% to 100% |
| ⌥-click a note | It splits there (where Find notes joined two) |
| Select notes ▸ **Correct to key** ▾ amount | Each moves toward the key's nearest note by that amount, and is straightened by it, as Melodyne's Correct Pitch does |

Every edit is one Tx and one undo step. Playback and export play the edited voice.

**The data.** An audio clip gains one field, `sung`, the notes found in its file:

```json
{"id": "z1b4r9td", "start": 64, "length": 4, "asset": "vox/lead.wav", "hash": "9f2c3a1be07d44c1", "offset": 0,
 "warp": [[1.25, 1], [1.62, 1.75]],
 "sung": [{"at": 1.25, "length": 0.37, "pitch": 64.31, "move": -0.31, "drift": 1},
          {"at": 1.70, "length": 0.52, "pitch": 66.92, "move": 0.08, "drift": 0.4}]}
```

| Field | Meaning |
|---|---|
| `at`, `length` | Where the note is **in the file**, in seconds. So the notes stay right when the clip is moved, copied, split or comped |
| `pitch` | The note as it was sung, as a MIDI number with cents: 64.31 is the E above middle C, 31 cents sharp. Find notes writes it; edits never change it |
| `move` | Semitones the note is moved by. 0 leaves it as sung |
| `drift` | 1 keeps the sung wobble. 0 makes the note one straight pitch |

Timing isn't in `sung`: it is the clip's warp markers, the time map music2 already has. So one mechanism moves a loop's
beats and a voice's notes, and a clip with moved notes follows the tempo, as Logic's Flex Time does (D6).

**Finding notes.** Two steps.
1. **Measure.** The core runs `@m2/vocals:pitch` offline over the clip's whole file and keeps its outputs, a frame
   every 256 samples. It is kept in the cache folder ([FMT-16](../../architecture.md#assets-and-hashes)),
   `<cache>/measures/<hash>.f32`, named by the file's hash, the kernel, its module's hash and its settings, as a
   stretch is named ([RENDER-30](../../architecture.md#audio-lanes)). Only the first look at a file costs time: a few
   seconds for a three-minute vocal. The window draws the pitch line from it.
2. **Find notes**, a tool in `@m2/vocals` written in TypeScript (§6), reads the measure. A note starts where the voice
   starts, where the pitch moves more than about 0.7 semitones and stays, or where it gets loud again. It ends where
   the voice stops. Its `pitch` is the middle value of its frames, weighted by loudness. Notes shorter than 60 ms join
   their neighbour. If these rules miss too often, Tony's note model (a hand-set hidden Markov model) can replace them
   in the same tool.

The song keeps the notes, not the measure. The measure can always be made again. An agent runs Find notes, and edits
notes, through the same routes the app uses.

**Playing the edits.** The clip is rendered offline, as a warped clip is today ([RENDER-30](../../architecture.md#audio-lanes)).
`std:stretch` learns a second method, **voice**, used when a clip has a `sung` note with a `move` or a `drift` other than as sung: TD-PSOLA from the same `m2-dsp` code
as `shift`, given the time map and each note's `move` and `drift`. Between notes the move eases over 30 ms. Two things
make editing feel instant:
- **Phrase by phrase.** The voice method renders each phrase (notes with gaps under 100 ms between them) on its own,
  from a fresh start, and caches each phrase by its own name. An edit makes again only the phrase it touched: a few
  seconds of audio, made in milliseconds. The rest plays from the cache. The same bits come out whole or by phrase,
  so an export is exact.
- **Its own pitch line.** The kernel tracks the pitch itself, with the same code as the measure, so it needs no input
  but the file. No new kind of kernel input.

A clip with no `sung` notes, or none edited, renders exactly as today.

## 5. Comping

**Using it**

| You do | What happens |
|---|---|
| Record round a loop | One clip, one file, one pass per time round (built: [MON-11](../../architecture.md#monitoring-overlays-and-mute-in-song)) |
| Click **Takes ▸** on the track | One row per pass opens under the lane. The part of each row that plays is lit |
| Swipe along pass 2 over bars 3 and 4 | Pass 2 plays there; the rest stays as it was. A 10 ms crossfade at each join. One undo step, "Comp on Vox" |
| Click a lit part of another row | That pass plays for that part |
| Close **Takes** | The lane shows the comp as clips |

**The data: nothing new.** A comp is plain audio clips side by side, each playing a different pass of the same file
by its `offset`, overlapping by 10 ms with fades. Before (an 8-beat loop at 120 bpm, three passes of 4 s, the newest
playing):

```json
{"start": 0, "length": 8, "asset": "vox take.wav", "offset": 8}
```

After swiping pass 2 over beats 2 to 4:

```json
{"start": 0,    "length": 2.01, "asset": "vox take.wav", "offset": 8,     "fade_out": 0.01},
{"start": 1.99, "length": 2.02, "asset": "vox take.wav", "offset": 4.995, "fade_in": 0.01, "fade_out": 0.01},
{"start": 3.99, "length": 4.01, "asset": "vox take.wav", "offset": 9.995, "fade_in": 0.01}
```

The Takes rows are worked out from what is there: clips that play one file whose note lists passes (`{"takes": [0,
4, 8]}`, [LIB-7](../../architecture.md#library)). No hidden state: delete the note, and the clips are plain clips.
Each piece keeps the clip's warp markers, moved as the Takes menu moves them today, and its `sung` notes, which are in
file time and so fit every pass.

**The gesture.** The swipe is drawn in the page while you drag. On release, the page asks the **Comp** tool (§6) for
the clips the lane should hold. The tool cuts at the swipe's edges, points the middle at the pass, adds the crossfades,
and joins neighbours that now play the same pass. The server writes the result as one `Set` Tx (the clip gesture
agents already use). An agent comps with the same tool.

## 6. What the core gains, and where everything lives

| The core gains | What | Count | Why TypeScript alone can't |
|---|---|---|---|
| New std kernels | none: `pitch`, `shift` and `snap` are `@m2/vocals`' own until a second pack plays them | 0 | (they are kernels, in a pack) |
| A std kernel changed | `std:stretch`'s **voice** method, rendered by phrase | 1 | The host renders clips offline from their files. That is the host's job and std's kernel; song code never touches a clip's samples |
| Kernel ABI | `k_latency`, reserved in v1, built | 0 new shapes | A latency in samples changes with the sample rate |
| Song format | `arrangement.json`'s `key` (moved from `layout.json`); an audio clip's `sung` | 2 fields | Edits must be data in the song (P2), and the key must reach sound |
| SDK | `time.key` | 1 signal | Build code may not read timeline data ([RENDER-15](../../architecture.md#events)); a signal isn't read by the build |
| **Measures** | The host runs one kernel offline over a clip's file and caches its outputs by hash; `GET /api/graph/clip/measure` gives them to the window and to agents | 1 mechanism, 1 route | A tool can't run a kernel; the window needs the pitch line too. Also wanted: slicing's onsets (gap 52), a clip's loudness |
| **Package tools** | A pack's TypeScript function, run in the sandbox when you click it, that is given a clip (its measure, the song's key) and returns the clips its lane should hold, written as one checked Tx; `POST /api/graph/tool`; a `tool` kind in `@m2/core`'s descriptors | 1 mechanism, 1 route | Today a click's logic is Rust in the server. This lets it be TypeScript in a pack. The contract's Deferred table has waited for this, "the first tool-shaped feature"; quantize, humanize, generators, slicing (gap 52) and an arpeggio written to a clip (gap #16) are tools too |

That is one std kernel changed, two fields, one signal, two mechanisms and two routes. Without package tools (D5), Find
notes, Correct to key and Comp would be three Rust gestures in the server instead: less new at first, but three
features in the core.

**`@m2/vocals` 1.0.0**, a new pack in `packs/@m2/vocals`, holds:

| Part | Kind |
|---|---|
| Tune | An effect, with its card (Speed, Amount) |
| `pitch`, `shift`, `snap` | Its own kernels, built by `std/build.sh` from `kernels-src/`, their shared parts in `m2-dsp` |
| Find notes, Correct to key | Tools on an audio clip's menu and in the Notes view |
| Comp | A tool behind the Takes rows. It works on any recorded audio, not only voices |

The app's own new code is the page: the Takes rows and the swipe, the roll over a waveform with sung notes, Tune's
card (drawn from its descriptor, as every card is).

## 7. "No AI in the app": how this note reads the rule

[goals.md](../../goals.md) says the app has no AI features. This note reads that as: **nothing learned from data
ships**, and nothing makes music for you. No neural network, no model trained on recordings (no CREPE-style pitch
finder, no neural voice). Every step is a formula you can read in the code: YIN, PSOLA, and note rules with numbers
written by hand. pYIN's and Tony's hidden Markov models, if used, are hand-set in the same way: their few probabilities
are written in the code, not trained. Tune and Find notes do only what you ask, on your own recording.

## 8. Plan

Each step is a track with a builder, a reviewer and a fix round.

| Order | Track | Builds | Size |
|---|---|---|---|
| 1 | `key` | The key moves into `arrangement.json`; `time.key`; `m2 upgrade` moves it; NOTE-1 and NOTE-2 point there | S |
| 1 | `tools` | Measures and package tools, their routes, the `tool` kind in `@m2/core`; first used by a test pack | M |
| 2 | `tune` | `@m2/vocals` 1.0.0 with Tune; `pitch`, `shift`, `snap`; `k_latency`. Tests: a gliding test tone lands within 5 cents of its notes, a vowel's formants stay put under a shift, latency is aligned, live equals export on 1 to 7 threads, nothing allocates on the audio thread | M |
| 2 | `comp` | The Comp tool; the Takes rows and the swipe; a browser suite | S |
| 3 | `sung` | The `sung` field; `std:stretch`'s voice method by phrase; Find notes and Correct to key; the roll over a waveform and its gestures; a browser suite | L |

`key` and `tools` run side by side; `tune` needs `key`; `comp` needs `tools`; `sung` needs `tools` and `tune`'s
`m2-dsp` blocks. The contract gets a section **Vocals** (VOX-), and changes RENDER-30 (the voice method), FMT-13 and
FMT-19 (the key), the audio clip's fields (`sung`), ABI-2 (`k_latency`) and the Deferred table's tools row.

## 9. Decisions for the owner

| # | Question | Recommendation |
|---|---|---|
| D1 | One new pack, `@m2/vocals`, for Tune, its kernels and the tools | Yes. Its kernels move to std only when a second pack plays them ([cards §7.1](../cards/design.md)) |
| D2 | Where the song's key lives | In `arrangement.json` beside the tempo, read by sound as `time.key`. (Instead: Tune gets its own key menu, filled in from the song's key when added, which doesn't follow a later change) |
| D3 | How `shift` moves pitch | TD-PSOLA, keeping formants, about 15 ms late. (Instead: zita-at1's way, reading faster or slower: a little less code, but the voice's colour moves with it, which shows in a harmony or a big move) |
| D4 | Tune's controls | Only Speed and Amount, the key from the song. Later, each when asked for: a Formant knob, following a drawn melody instead of the key (its notes lane as an events input, [ABI-8](../../architecture.md#kernel-abi-reservations)), a Humanize |
| D5 | Find notes, Correct to key and Comp as TypeScript tools in the pack, which needs package tools | Yes: one general mechanism instead of three features in the core, and the Deferred table's trigger has come. (Instead: three Rust gestures) |
| D6 | A note's timing edits as warp markers | Yes: one time map for loops and voices. A clip whose notes moved then follows the tempo, as Flex Time does |
| D7 | How edited notes are rendered | TD-PSOLA in `std:stretch`, phrase by phrase. A vocoder like WORLD (BSD) only if PSOLA sounds wrong on big moves |
| D8 | What can be comped | The passes of one loop recording now. Comping separate recordings (each Record over the same bars) later: it needs Record to keep the old take as a pass |
| D9 | One voice at a time | Yes: Find notes and Tune work on one sung line. Chords inside a recording (Melodyne's DNA, patented) are not planned |

## 10. Alternatives

| Instead | Why not |
|---|---|
| One big Tune kernel with everything inside | Its parts are wanted alone: a tuner, harmony, formant, slicing |
| Edits as a control lane of semitones on the track | A lane doesn't move with its clip: drag the vocal and its fixes stay behind |
| Keep the measure in the song, not the notes | It is large and can be made again. The notes are small, and they are what you edited |
| Render edits live, through `shift` on the track | The edits belong to the clip, and the clip renders offline already. Offline also sees the whole phrase, so the pulses are found better |
| A new kind of take folder in the format | Plain clips already play passes by their offset. A folder would be a second way to say the same thing |
| Host Auto-Tune or Melodyne | They don't ship CLAP, as far as is known, and music2 is CLAP only (gap #33) |
