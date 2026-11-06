# Vocals: tune a voice, edit its notes, comp the best take

| | |
|---|---|
| **Status** | Proposed, for the owner's yes (AGENTS.md rule 11). Nothing here is built. Revised after the skeptic's review (V1 to V23). Rules: VOX-1 to VOX-n in the contract, written by the tracks of §8. |
| **Written** | 2026-11-06, from `main` at 8d37e209 |
| **Serves** | gap 50 (pitch correction), gap 51 (editing a vocal's notes), gap #25 (comping). It also gives gap 52 (slicing), gap 54 (vocal sounds) and gap #16 (the key in code) parts they need. Gaps 50 to 54 are on branch `gaps2`, not yet on `main` |

**In short**
- **Tune** is a new effect for a voice. It moves each sung note onto the nearest note of the song's key. One knob,
  **Glide**, goes from "Robot" (the hard, stepped sound of pop vocals) to "Natural". It works while you sing and on a
  recorded clip, and an export sounds the same as playback.
- **A voice's notes** can be edited like the piano roll's. **Find notes** turns a recorded vocal into notes you can
  see. You drag a note up to fix its pitch, or sideways to fix its timing. Each edit is saved in the clip's own data.
- **Comping** builds one take from the best parts of several. You record a line a few times round a loop, then swipe
  across the take you like for each part. One undo step, and it saves only plain audio clips.
- **Classic sound maths only**, nothing learned from data (§7).
- **The core stays small** (§6): no new std kernel. It gains three fields in the song's files, one time signal, a
  shape of offline kernel that measures, a way for a pack's kernel to render a clip, and small changes to two
  routes. The sound work lives in a new pack, `@m2/vocals`.

## Words used here

| Word | Meaning |
|---|---|
| **Pitch** | How high or low a sound is. A sung note has one; a "s" or "t" has none. |
| **Semitone** | The step from one piano key to the next. 100 **cents** make a semitone. |
| **Key** | The song's set of notes that sound right together, such as A minor ([NOTE-1](../../architecture.md#notes-and-scales)). |
| **Formants** | The "vowel" colour of a voice. If they move with the pitch, a voice sounds like a chipmunk or a giant. |
| **Latency** | How late a sound comes out of an effect. Music2 delays the other tracks to match ([RENDER-18](../../architecture.md#latency)). |
| **Take**, **pass** | One recording of a part. Recording round a loop makes one pass per loop, all in one file ([MON-11](../../architecture.md#monitoring-overlays-and-mute-in-song)). The app calls each one "Take 1", "Take 2". |
| **Comp** | One take made from the best parts of several. |
| **Kernel** | A small program, in WebAssembly, that works on sound sample by sample. TypeScript connects kernels but can't do that work itself. An **offline** kernel works on a whole file, away from playback. |

## 1. What a pop producer needs

| Need | What they use today | In music2 |
|---|---|---|
| A voice locked to the key, softly or as a hard effect | Auto-Tune, Logic's Pitch Correction | **Tune**, an effect (§3) |
| One wrong note fixed, a line moved in time | Melodyne, Logic's Flex Pitch | **Notes** of an audio clip (§4) |
| The best line from five takes | Logic's take folders, Ableton's take lanes | **Comp** across a loop's takes (§5) |

The big tools (Auto-Tune, Melodyne) aren't CLAP plugins, as far as is known, so music2 can't host them (gap #33).

## 2. How open tools do it

| Tool | Finds the pitch by | Moves it by | What we take | Licence |
|---|---|---|---|---|
| [zita-at1](https://kokkinizita.linuxaudio.org/linuxaudio/zita-at1-doc/quickguide.html), and [x42's fat1](https://github.com/x42/fat1.lv2) built on it (`src/retuner.cc`) | Autocorrelation through an FFT, every 4 blocks of 128; 60 to 1,200 Hz; "voiced" only when it matches above 0.8 | Reading the input faster or slower, jumping by a whole pitch period with a short crossfade; about 27 ms late (half in its low-latency mode) | The nearest allowed note, a **bias** that keeps the current note, a correction that jumps when the note changes, an **amount** | GPL-2 |
| Autotalent (Tom Baran; [Debian's package](https://packages.debian.org/sid/autotalent)) | Autocorrelation | Overlap-add of short grains | Allowed notes, strength, and **formant correction** | GPL |
| [Rubber Band](https://breakfastquay.com/rubberband/technical.html) | (none) | A phase vocoder; pitch as a resample plus a stretch the other way | That a stretch and a pitch shift are one job. `std:stretch` already does it for loops (WSOLA) | GPL or paid |
| [Praat](https://www.fon.hum.uva.nl/praat/manual/Manipulation.html)'s Manipulation | Its pitch analysis, then one mark per vocal pulse | Overlap-add from the sound, its pulses, a **pitch tier** and a **duration tier** | The model for editing notes: the sound stays, the edits are small lists, rendered from them | GPL |
| [WORLD](https://github.com/mmorise/World) (Morise) | Harvest or DIO | Splits a voice into pitch, colour and noise, then builds it again | A better method for big moves, if PSOLA isn't enough | Modified BSD |
| [pYIN](https://code.soundsoftware.ac.uk/projects/pyin) and Tony (Mauch and Dixon, 2014; Mauch et al., 2015) | YIN with several guesses per moment, smoothed by a small table of hand-set chances (a hidden Markov model) | (none) | How to turn a pitch line into notes | GPL |

The methods are published: YIN (de Cheveigné and Kawahara, *JASA* 111(4), 2002) and TD-PSOLA (Moulines and
Charpentier, *Speech Communication* 9, 1990). music2 is MIT or Apache-2.0, so it copies no GPL code: each kernel is
written from the papers. Auto-Tune's first patent ([US 5,973,252](https://patents.google.com/patent/US5973252), filed
1998) is listed as expired. Melodyne's chords-in-a-mix method (DNA) is patented, and music2 doesn't try it (D10).

## 3. Tune: pitch correction as an effect

**Using it**

| You do | What happens |
|---|---|
| + Effect ▸ **Tune** on the Vox track | `tune(…)` is written into the track's code, as any effect is. It opens on **Natural** |
| The card's **Robot** preset | Glide 0: each note jumps to its pitch at once, the stepped pop sound |
| Glide to about 0.1 s | A gentle fix: each note slides to its pitch, and vibrato stays |
| Amount to 50% | Half of each note's error is fixed |
| No key set | Every note is allowed, and the card says "No key set: every note is allowed. Set the key" |
| Sing with Hear yourself on ([MON-8](../../architecture.md#monitoring-overlays-and-mute-in-song)) | You hear yourself tuned. The whole song is about 15 ms later while Tune is on (D6). The take records your dry voice |
| Export | The same sound as playback, bit for bit ([RENDER-1](../../architecture.md#offline), [RENDER-2](../../architecture.md#offline)) |

The card has two knobs, **Glide** (0 to 0.4 s, its ends named "Robot" and "Natural") and **Amount**, and the song's key
in words.

**How it works.** Three kernels in a row, joined in TypeScript in `@m2/vocals` (a sketch, not final code):

```ts
export const tune = component({ id: "tune", label: "Tune", /* … */
    controls: { glide: control.secs(0.08).range(0, 0.4), amount: control.frac(1) } },
  (x, { glide, amount }) => {
    const sung = kernel("@m2/vocals:pitch", { in: x });                     // its pitch, and how sure
    const by = kernel("@m2/vocals:snap", { ...sung, key: time.key, glide, amount }); // semitones to move
    return kernel("@m2/vocals:shift", { in: x, semitones: by, hz: sung.hz });  // moved, formants kept
  });
```

| Kernel | What it does | Method |
|---|---|---|
| `pitch` | Finds the voice's pitch (`hz`), how sure it is (`clarity`, 0 to 1) and its loudness, every 256 samples, from the mono sum of its channels | YIN: the difference between the sound and itself one period later, found through an FFT, then a parabola through the lowest dip for the exact period. 70 to 1,200 Hz; outside that it says "no pitch". It looks only at the past, so it adds no latency |
| `snap` | Works out how many semitones to move: from the sung pitch to the nearest note of the key, a bias of 0.2 semitones keeping the current note, eased over `glide`, times `amount`. Where `clarity` is low (an "s", a breath), it eases back to 0. It reads `time.key` only as a whole number and keeps the last good one | Plain maths with a little memory. It is the only part made just for Tune. A pitch reported an octave off gives the same answer, as it is the same note name |
| `shift` | Moves the pitch by `semitones` and keeps the formants. A `formant` input moves the colour on its own (0 for Tune) | TD-PSOLA: grains two periods long, one around each vocal pulse, laid out again at the new spacing. A grain keeps its shape, so the vowel stays. **Pulses**: the largest peak of the signal low-passed at 1 kHz within a quarter period of where the last pulse plus `hz`'s period says, so marks stay steady from period to period; a period that disagrees with the last marks by a factor near 2 (an octave error) is taken as the one nearer the last. Where `clarity` is low or a mark can't be found, the sound passes through, delayed by the same look-ahead, with a 10 ms crossfade, so a switch never moves time. Both channels use the same marks, so a stereo image stays put. It looks one period ahead: 15 ms |

All buffers are sized in `k_new` from the settings' rate (about 43 ms of history and look-ahead: 2,060 frames at
48 kHz, twice that at 96 kHz), so nothing is taken while it plays.

**Why kernels, not TypeScript.** TypeScript in music2 builds the graph once; it never sees a sample. Finding a
period, keeping a note in memory and laying out grains are work on samples. std's math kernels (`add`, `scale`,
`mtof`…) have no logarithm and no grains, so the chain can't be built from them.

**Why three kernels, not one.** Each is useful alone. `pitch` serves a tuner, a voice that plays a synth, and slicing
by note (gap 52). `shift` serves a harmony, an octave effect and formant shifting (gap 54). Only `snap` is Tune's own.
All three are `@m2/vocals`' own kernels, in one module. Each moves to std only once a second pack plays it
([cards §7.1](../cards/design.md)).

**The song's key, read by sound.** Today the key lives in `layout.json`, which code never reads (gap #16,
[FMT-19](../../architecture.md#presentation-and-local-files)). A key is part of the music, like the tempo, so it moves
into `data/arrangement.json` as `"key": {"root": "A", "scale": "minor"}` or `null`. One key for the whole song; key
changes are a later gap of their own (D2).
- Sound code reads it as **`time.key`**, a signal like `time.playing`, of a new unit, `keyset`: a whole number whose bit
  n (0 is C) is on when that note is in the key. No key is 4095: all twelve on.
- It is a signal, not something the build reads ([RENDER-15](../../architecture.md#events)), so changing the key
  rebuilds nothing. Changing it is now song data: an undo step, in a revision.
- A scale name music2 doesn't know (a typo, a newer music2's) is a Warning, `arrangement.key-unknown`, and means no
  key. It never stops a song from playing.
- NOTE-1 and NOTE-2 read the key from the arrangement instead of `layout.json`. `m2 upgrade` moves an old song's key.

**Latency.** Today a kernel's latency is a number of frames in its manifest. `shift`'s is a time, so the manifest
gains `latency_s = 0.015`, which the host rounds up to frames at the song's rate, by one rule the kernel kit uses too.
The check stays as it is (a latent kernel inside a `poly` is still refused), and `k_latency` stays reserved for a
latency that depends on a kernel's settings. The other tracks are delayed to match
([RENDER-18](../../architecture.md#latency)); `pitch` then sees about 15 ms ahead of the sound `shift` moves, so the
correction lands on time.

**Recording under latency.** With Tune on any track, the song's total latency is 15 ms, on every vocal. The take's
start is placed today without that latency (`crates/server/src/graph/record.rs`), so the `tune` track first adds the
test `a_take_under_a_latent_song_lands_where_it_was_heard` and fixes the placing if it fails.

## 4. A voice's notes: edit pitch and timing

**Using it**

| You do | What happens |
|---|---|
| Right-click a vocal clip ▸ **Find notes** | music2 finds each sung note and opens the clip in the Notes view. One undo step. On a song that doesn't list `@m2/vocals` yet, it is added to the song in the same step, as adding any pack's item is ([PACK-10](../../architecture.md#packs)) |
| Look | The piano roll, with the clip's waveform faint behind. Each sung note is a bar at its pitch, as thick as it is loud, with the sung pitch line drawn through it. Notes outside the key are shaded ([NOTE-2](../../architecture.md#notes-and-scales)) |
| Drag a note up or down | It moves by whole semitones; with ⌥ held, by cents; with Lock to key, onto the key ([NOTE-3](../../architecture.md#notes-and-scales)) |
| Drag a note sideways, or its edge | Its timing moves. Only that note and the gaps beside it change (D7) |
| Double-click a note | It moves to the nearest note |
| **Straighten** (a slider in the note's menu) | Its wobble and vibrato are flattened, from 0% to 100% |
| ⌥-click a note | It splits there (where Find notes joined two) |
| **This note is…** (its menu) | Says what note was really sung, where Find notes heard it wrongly (an octave off) |
| Select notes ▸ **Correct to key** ▾ amount | Each moves toward the key's nearest note by that amount, and is straightened by it, as Melodyne's Correct Pitch does |

Every edit is one Tx and one undo step. The roll's edits are worked out in the page, the key's notes from the
table the timeline publishes ([NOTE-2](../../architecture.md#notes-and-scales)), and sent as the clip written whole
(the `Set` clip gesture that exists). Playback and export play the edited voice.

**The data.** An audio clip gains two fields:

```json
{"id": "z1b4r9td", "start": 64, "length": 4, "asset": "vox/lead.wav", "hash": "9f2c3a1be07d44c1", "offset": 0,
 "render": "@m2/vocals:voice",
 "sung": [{"at": 1.25, "length": 0.37, "pitch": 64.31, "move": -0.31, "straighten": 0},
          {"at": 1.70, "length": 0.52, "pitch": 66.92, "move": 0.08, "straighten": 0.6}]}
```

| Field | Meaning |
|---|---|
| `render` | The offline kernel that plays the clip's edited notes. Find notes writes it |
| `at`, `length` | Where the note is **in the file**, in seconds |
| `pitch` | The note as sung, as a MIDI number with cents: 64.31 is the E above middle C, 31 cents sharp. Find notes writes it; **This note is…** changes it |
| `move` | Semitones the note is moved by. 0 leaves it as sung |
| `straighten` | 0 keeps the sung wobble; 1 makes the note one straight pitch, as the slider says |

**What plays**, at each moment of a note: the sung pitch line plus `move`, pulled toward `pitch + move` by
`straighten`. So the bar the roll draws, at `pitch + move`, is where a straightened note sounds. A note with `move`
0 and `straighten` 0 sounds as sung, whatever its `pitch` says.

**Timing** is the clip's warp markers, the time map music2 already has. A timing drag writes markers at the note's
start and end, and keeps fixed markers at the end of the note before and the start of the note after (and, on a clip
with no markers yet, at the clip's start and end). So only that note and its two gaps stretch, as Logic's Flex Time
does. The first timing edit makes the clip follow the song's tempo (D7).

**Split, copy, comp, find again** (D8, D12):
- A clip keeps only the `sung` notes in the part of the file it plays. Split, comp and resize trim the list; a note
  across a cut stays in both pieces, and the roll edits it in both in one Tx.
- A copy is its own clip: fixing a note in one copy of a chorus doesn't fix the other.
- Running Find notes again keeps your edits: each edited note's `move` and `straighten` go to the new note that
  overlaps it by more than half.

**Finding notes.** Two steps, with all the musical rules in the pack:
1. **Measure.** The pack's offline kernel `@m2/vocals:notes` reads the clip's whole file (the mono sum of its
   channels) and writes, every 256 samples: `hz`, `clarity`, `level`, and `note`, which is 0 between notes and the
   note's number (1, 2, 3…) inside one, with `centre`, that note's sung pitch. It reads its file itself, so it can look
   ahead: a note starts where the voice starts, where the pitch moves more than about 0.7 semitones and stays, or where it
   gets loud again; it ends where the voice stops; its `centre` is the middle value of its pitch, weighted by loudness;
   notes under 60 ms join a neighbour. If these rules miss too often, Tony's note rules (a small table of hand-set
   chances) can replace them in the same kernel. The result is kept in the cache folder
   ([FMT-16](../../architecture.md#assets-and-hashes)), `<cache>/measures/<hash>.f32`, named by the file's hash, the
   kernel, its module's hash and its settings. Only the first look at a file costs time.
2. **The Find notes gesture** (in the core, a few lines) turns each run of one `note` number into a `sung` note: its
   `at`, `length` and `pitch` (the run's `centre`). It also writes `render` and, when needed, the song's dependency.

The window draws the pitch line from the same measure (`GET /api/graph/clip/measure`), and agents read it there too.
The song keeps the notes, not the measure, which can always be made again.

**Playing the edits.** A pack's offline kernel renders the clip's edited notes: `@m2/vocals:voice`, named by the
clip's `render`. It is given the file as asset 0, the clip's time map and its `sung` notes as values (five numbers
each). It uses TD-PSOLA and tracks the pitch with the same code as the measure, in the same module, so the line it
moves is the line the roll draws. std gains nothing, and a fix to the pack's PSOLA moves only the pack's version.

Only **edited phrases** go through it; everything else plays as today. The proposed contract text:

> **RENDER-30b.** An audio clip MAY name a pack's offline kernel in `render`; it plays the clip's **edited phrases**
> only. An edited note is a `sung` note whose `move` or `straighten` isn't 0. Its region runs from 50 ms before it to
> 50 ms after it, in the file's seconds; regions that overlap or are less than 20 ms apart are one **phrase**.
> Outside its phrases, the clip plays its normal path (a plain read, or `std:stretch` when warped), so a clip with one
> edit plays the same bits as with none everywhere but that phrase.
> - Each phrase is rendered alone, from its kernel's init: the kernel reads from 100 ms before the region (its
>   lead-in) and writes the region only, laid out by the clip's time map and transposed by the clip's `pitch`. It is
>   one file, `<cache>/renders/<hash>.f32`, named by the source's hash, the kernel's module's hash, the region, the
>   lead-in, the time map's pairs that reach it, its notes (every field), the clip's `pitch` and the song's rate.
> - The phrase replaces the normal path over its region, with a 10 ms raised-cosine crossfade at each end, inside the
>   50 ms margin, where no edit is heard.
> - **Offline**, every phrase is made whole before the render plays it, as a stretch is. **Live**, phrase jobs go ahead
>   of other stretches on the stretch thread, and until a phrase's new file is whole, the lane plays what it played
>   before: the phrase's last render, or for a first edit the normal path. It switches at the next chunk edge, and
>   that chunk fades from old to new over its first 5 ms. An edit never plays silence or counts a miss.
>
> Tested by: an edit while playing counts no miss (the `sung` track's browser suite); after any order of edits, once
> made, the live lane plays the export's bits; an unedited phrase plays the bits of the clip with no `render`.

## 5. Comping

**Using it**

| You do | What happens |
|---|---|
| Record round a loop | One clip, one file, one take per time round (built: [MON-11](../../architecture.md#monitoring-overlays-and-mute-in-song)) |
| Click **Takes ▸** on the track | One row per take opens under the lane ("Take 1", "Take 2"…). The part of each row that plays is lit |
| Swipe along Take 2 over bars 3 and 4 | Take 2 plays there; the rest stays. A 10 ms crossfade at each join. One undo step, "Comp on Vox" |
| Click a dim part of another row | That take plays for that part |
| Close **Takes** | The lane shows the comp as clips |

**The data: nothing new.** A comp is plain audio clips side by side, each playing a different take of the same file
by its `offset`, overlapping by 10 ms with fades. Fades are raised cosines, so over the overlap they add up to 1. Before
(an 8-beat loop at 120 bpm, three takes of 4 s, the newest playing):

```json
{"start": 0, "length": 8, "asset": "vox take.wav", "offset": 8}
```

After swiping Take 2 over beats 2 to 4:

```json
{"start": 0,    "length": 2.01, "asset": "vox take.wav", "offset": 8,     "fade_out": 0.01},
{"start": 1.99, "length": 2.02, "asset": "vox take.wav", "offset": 4.995, "fade_in": 0.01, "fade_out": 0.01},
{"start": 3.99, "length": 4.01, "asset": "vox take.wav", "offset": 9.995, "fade_in": 0.01}
```

- The rows are worked out from what is there: clips that play one file whose note lists its takes (`{"takes": [0, 4,
  8]}`, [LIB-7](../../architecture.md#library)). A piece's take is the one that holds the middle of the part of the
  file it plays. No hidden state: delete the note, and the clips are plain clips.
- Each piece keeps the clip's warp markers, moved as the Takes menu moves them, and its `sung` notes, trimmed (§4).
- The overlap is in beats and the fades in seconds, so Comp works the overlap out from the tempo at the cut. A later
  tempo change leaves it a little off (at 60 bpm, 20 ms of overlap with 10 ms fades) until the comp is next touched,
  when Comp puts it right. That small error goes to `docs/gaps.md`.

**The gesture is in the core**, beside the Takes menu it extends: `POST /api/graph/take` (Rust, MON-11) takes an
optional range, `from` and `to` in beats. It cuts the clip at the range's edges, points the middle at the take,
adds the crossfades, and joins neighbours that now play the same take: one Tx, one undo step (MON-14). Recording is
the core's, so comping needs no pack, and agents comp through the same route.

## 6. What the core gains, and where everything lives

| The core gains | What | Count | Why TypeScript alone can't |
|---|---|---|---|
| std kernels | none: `pitch`, `shift`, `snap`, `notes` and `voice` are `@m2/vocals`' own | 0 | (kernels, in a pack) |
| Offline kernels | may write controls at a hop instead of audio (a **measure**, `notes`); `check_offline` takes that shape and `offline.rs` runs and caches it | 1 shape | Finding notes needs the whole file and samples |
| Clip rendering | a clip's `render` names a pack's offline kernel for its edited phrases; phrases joined to the normal path (RENDER-30b, in `core/runtime`) | 1 mechanism | The host renders clips from their files; song code never touches a clip's samples |
| Kernel manifest | `latency_s` | 1 field | A latency in frames changes with the sample rate |
| Song format | `arrangement.json`'s `key` (moved from `layout.json`); an audio clip's `sung` and `render` | 3 fields | Edits must be data in the song (P2), and the key must reach sound |
| SDK | `time.key`, of the new unit `keyset` | 1 signal, 1 unit | Build code may not read timeline data ([RENDER-15](../../architecture.md#events)) |
| Routes and gestures | the Takes route takes a range (comp); the Find notes gesture; `GET /api/graph/clip/measure` | 1 new route, 2 changed | Rust beside the code they extend |
| A fix | where a take lands under a latent song, if its test fails | — | — |
| The page | the Takes rows and the swipe; the roll for an audio clip; Tune's card (from its descriptor, as every card is) | the app's own | — |

**Package tools** (a pack's TypeScript run on a click, which the contract's Deferred table holds back) aren't needed:
Comp and Correct to key are small and belong in the core and the page, and Find notes' rules live in the pack's
measure kernel. They stay deferred until a feature needs them.

**`@m2/vocals` 1.0.0**, a new pack in `packs/@m2/vocals`:

| Part | Kind |
|---|---|
| Tune | An effect, with its card (Glide, Amount) and presets Natural and Robot |
| `pitch`, `snap`, `shift` | Live kernels |
| `notes`, `voice` | Offline kernels: the measure, and the renderer of edited notes |

They are one module, built by `std/build.sh` from the pack's `kernels-src/`
([ABI-10](../../architecture.md#kernel-abi-reservations)). Parts other packs may want later go in `m2-dsp`.

## 7. "No AI in the app": how this note reads the rule

[goals.md](../../goals.md) says the app has no AI features, and the contract's
[AGENT-9](../../architecture.md#agents) says "the app MUST contain no model and call none". This note reads "model"
as **a model learned from data**: no neural network, nothing trained on recordings (no CREPE-style pitch finder, no
neural voice), no network call, and nothing made for you. Every step is a formula or a rule you can read in the code:
YIN, PSOLA, and note rules with numbers written by hand. Tony's note rules, if used, are a small table of chances
written by hand, not trained. That reading is the owner's to make (D11); if yes, AGENT-9's *Why* gets that line.

## 8. Plan

Each step is a track with a builder, a reviewer and a fix round. Each updates the guide (`docs/guide/`: Tune,
editing a voice's notes, comping) and `docs/gaps.md` where it changes what a musician can do.

| Order | Track | Builds | Size |
|---|---|---|---|
| 1 | `comp` | The Takes route's range; the Takes rows and the swipe; a browser suite | S |
| 1 | `key` | The key in `arrangement.json`, `arrangement.key-unknown`, `time.key` and `keyset`, NOTE-1 and NOTE-2 moved, `m2 upgrade`; a key change while playing moves Tune at once and the export follows it | S |
| 2 | `tune` | `@m2/vocals` 1.0.0 with Tune, `pitch`, `snap`, `shift`; `latency_s`; the take test. Its tests measure pitch by another method (an FFT peak) than `pitch`; play a made-up singer (a pulse train with 5.5 Hz vibrato through three formant filters for "ah", and a burst of noise for "s"); check that at Glide 0 a gliding voice steps within one hop of each note change, that the formants stay within 5% under a 4-semitone shift, that the "s" passes through, that latency stays aligned, that live equals export on 1 to 7 threads, and that nothing allocates while it plays at 44.1, 48 and 96 kHz. A short CC0 pop vocal joins the fixtures, with an owner's by-ear check owed | L |
| 3 | `sung` | Measures, `notes` and `voice`, RENDER-30b, `sung` and `render`, Find notes, the roll over a waveform and its edits; tests: a note sung 31 cents sharp, straightened, plays at 64.31 by another measure; a timing drag stretches only its gaps; RENDER-30b's three tests; a browser suite | L |

`comp` and `key` run side by side; `tune` needs `key`; `sung` needs `tune`'s PSOLA. The contract gets a section
**Vocals** (VOX-), RENDER-30b, and changes to FMT-13 and FMT-19 (the key), the audio clip's fields, the kernel
manifest (`latency_s`), MON-11 (the range) and AGENT-9's *Why* (if D11 is yes).

## 9. Decisions for the owner

| # | Question | Recommendation |
|---|---|---|
| D1 | Put the vocal tools in one new add-on pack, `@m2/vocals`, so music2's core doesn't grow? | Yes |
| D2 | Keep the song's key with the tempo, so sounds like Tune can read it, and allow one key per song for now? | Yes. Key changes (a lift in the last chorus) later, as their own gap |
| D3 | With no key set, Tune allows every note and its card says "Set the key"? | Yes |
| D4 | Tune has only two knobs, Glide and Amount, opens on Natural, and has a Robot preset? | Yes. A Formant knob, following a drawn melody instead of the key, and Humanize come later, each when asked for |
| D5 | Tune moves pitch the way that keeps the voice's own colour even for big moves (PSOLA), which harmony can use later? Or a simpler way that is fine only for small fixes? | The way that keeps the colour |
| D6 | With Tune on, the whole song plays about 15 ms later, so while recording you hear yourself a little later. Accept that for now? | Yes. If it bothers a singer, Tune can be off while recording; a no-delay recording mode later if singers ask |
| D7 | Moving a note in time makes its clip follow the song's tempo from then on, as Logic's Flex Time does? | Yes |
| D8 | Running Find notes again on a clip you edited keeps your edits? | Yes |
| D9 | Comp the takes of one loop recording now, and separate recordings later? | Yes |
| D10 | One voice at a time: no editing chords inside a recording? | Yes |
| D11 | "No AI" means nothing learned from data and nothing made for you; rules and tables written by hand are fine? | Yes, and AGENT-9 says so |
| D12 | Copies of a clip are separate: fixing a note in one copy of a chorus doesn't fix the others? | Yes: simple, and what you see is what plays |

## 10. Choices made for the lead

| What | Choice | Instead |
|---|---|---|
| Who renders edited notes | A pack's offline kernel, named by the clip's `render` (V1) | `std:stretch` with a second method: std's version would move with every fix to the pack |
| How edits play | Phrase by phrase, exactly as RENDER-30b (V2, V3) | The whole clip on each edit: slow, and silent while live |
| Find notes | The pack's measure kernel and a small core gesture (V4, V5) | Package tools: a sandbox, a descriptor kind and a route for one user |
| Measures | An offline kernel that writes controls at a hop (V14) | Running a live kernel over a file: a third way to run kernels |
| Latency | `latency_s` in the manifest (V6) | Building `k_latency`: the check would lose its refusal inside a `poly` |
| Comp | The Takes route with a range, in Rust (V4) | A pack's tool: recording would depend on a pack |
| Correct to key | A roll edit from NOTE-2's table | A tool |

## 11. Alternatives

| Instead | Why not |
|---|---|
| One big Tune kernel with everything inside | Its parts are wanted alone: a tuner, harmony, formant, slicing |
| Edits as a control lane of semitones on the track | A lane doesn't move with its clip: drag the vocal and its fixes stay behind |
| Keep the measure in the song, not the notes | It is large and can be made again. The notes are small, and they are what you edited |
| Render edits live, through `shift` on the track | The edits belong to the clip, and offline sees the whole phrase, so pulses are found better |
| A new kind of take folder in the format | Plain clips already play takes by their offset |
| Host Auto-Tune or Melodyne | They don't ship CLAP, as far as is known, and music2 is CLAP only (gap #33) |
