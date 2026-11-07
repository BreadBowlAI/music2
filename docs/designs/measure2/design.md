# Pictures measure the real sound

| | |
|---|---|
| **Status** | Proposed, for the owner's yes (AGENTS.md rule 11). Nothing is built. |
| **Written** | 2026-11-07, from `main` at `c653658c`, track `measure2-note` |
| **Builds on** | [Cards you play with](../cards2/design.md) §4.7 and D13 ("measure where you can"), and the contract's [Curves measured from the sound](../../architecture.md#curves-measured-from-the-sound) (CARD-80 to CARD-86) |
| **Changes** | the pictures of std, `@m2/mixing`, `@m2/effects`, `@m2/synths` and `@m2/wavetable` that draw from copied sums (CARD-93, CARD-96, CARD-97, CARD-113, CARD-115 to CARD-118, CARD-121 to CARD-124, CARD-131 to CARD-133, CARD-136, CARD-137, CARD-140) |
| **Prototype** | `$SCRATCH/measure2-note/proto_tests.rs`: the costs in §6, timed on the Linux server |

**In short**
- **Today about thirteen pictures copy their sound's maths into TypeScript.** The reverb's tail, the delay's echoes, the compressor's curve, drive, widen, the bitcrusher, the transient shaper, the multiband's split, Pump's dip, the tremolo, the 24 dB ladder, the Wavetable's frames and filter, the FM partials, the envelope. Each is a second copy of the truth, kept honest only by a test that renders the sound and checks the picture within 0.5 dB. About 900 lines of copied sums, and about 700 lines of tests that guard them.
- **The proposal: one general step, `measure`.** A picture says what to play into a copy of its sound (a step, a tone, or a note) and what to read from what comes out (its levels over time, its wave, or its spectrum). music2 plays the real kernels offline, as `p.response()` and `p.hit()` already do, and hands the numbers to the picture's `draw`. The picture then draws what the sound really did.
- **It replaces `p.response()` and `p.hit()`.** Those two become a few lines of std TypeScript over `measure`. The core's word list loses two words and gains one field with three kinds of input and three kinds of reading.
- **It is cheap enough.** Measured (§6): most pictures take 1 to 5 ms of CPU at 48 kHz. The worst is the biggest reverb, whose 12 s tail takes 26.5 ms to play. The compressor's curve at 13 levels takes 4.2 ms. A Wavetable note takes 8.5 ms, almost all of it the kernel making its table.
- **While you drag,** a measure is kept by what it plays, so a value that doesn't change the sound measures nothing. A long sound that can't be played inside a frame draws what it heard so far, and is measured whole when you let go.
- **What stays drawn from the values:** the dots, the guide lines, an envelope's corners, an LFO's named shape, the de-esser's band. The chorus's and the phaser's motion keep a small copy for now (D7).
- **The core gains no kernel, no route, no descriptor field and no engine change.** The offline copy (CARD-81) learns to play a tone and steps of levels, to set a few of its values differently, and to read three ways. About 500 lines of Rust and TypeScript. The packs lose about 900 lines of sums, and the pictures become right at 44.1 and 96 kHz too, where today several are worked out for 48 kHz only.

## 1. What is wrong today

The cards2 design (§4.7, D13) said: a curve that can be measured is measured, and the pack's own maths is for what a click can't measure. A click (`p.response()`) can measure only a sound that stays the same at every level and every time, and that rings for under 2 s. So everything else was drawn from sums the pack copied out of its kernel:

| Picture | Rule | What the pack copies | Its test against a render |
|---|---|---|---|
| Reverb's tail | CARD-118 | Freeverb's eight combs and four all-passes, their lengths at 48 kHz, 32 frequency bands, how the combs cancel in the all-passes (about 240 lines) | `the_reverbs_tail_is_its_sound` |
| Transient shaper's hit | CARD-113 | its followers, its 25 ms window, its glide, each share, summed in closed form (about 180 lines) | `the_transient_shapers_hit_is_its_sound` |
| Wavetable's frames | CARD-131, CARD-132 | the five built-in tables' harmonics, the vowels' formants, the kernel's mixing of frames (about 170 lines) | `the_drawn_tables_are_the_wavetables_sound` |
| FM synth's partials | CARD-140 | Bessel functions, the index limit at 48 kHz, the tone's brightness, fold-back (about 80 lines) | `the_drawn_partials_are_the_fm_synths_sound` |
| Multiband's split | CARD-115 | Linkwitz-Riley crossovers and the all-pass (about 45 lines) | `the_multiband_split_is_its_sound` |
| Polysynth's ladder, Wavetable's filter | CARD-133, CARD-136 | the ladder's feedback curve, four state-variable shapes (about 55 lines) | `the_drawn_ladder_is_the_polysynths_filter`, `the_drawn_filter_is_the_wavetables_filter` |
| std's drawn lowpass | CARD-97 | a two-pole lowpass (about 45 lines); no shipped sound uses it now | `the_drawn_lowpass_is_stds_filter_up_to_a_twelfth_of_the_rate` |
| Drive's curve, bitcrusher's sine | CARD-122, CARD-123 | `tanh`, the crusher's clock and rounding (about 40 lines) | `drives_curve_is_its_sound`, `the_bitcrushers_sine_is_its_sound` |
| Compressor's curve | CARD-93 | the knee, the ratio, the makeup (about 25 lines) | `the_compressors_transfer_curve_is_its_sound` |
| Envelope | CARD-96 | adsr's 60 dB falls (about 25 lines) | `the_envelope_is_adsrs_sound` |
| Tremolo, delay, Pump, widen | CARD-121, 117, 116, 124 | a cosine, a feedback power, the dip's power, the made-up width (about 35 lines) | five tests |
| Chorus's and phaser's motion | CARD-119, CARD-120 | 8 ms and 6 ms; 640 Hz and 2.5 octaves | two tests |

What that costs:
- **Two truths.** Change a kernel and its picture lies until someone changes the TypeScript too. The 0.5 dB tests catch it, but only for the settings each test tries.
- **48 kHz only.** The effects', the transient shaper's, the split's and the FM partials' sums are worked out at 48 kHz (CARD-110, CARD-140). A song at 96 kHz is drawn a 48 kHz sound.
- **What can't be copied isn't drawn.** A Wavetable table loaded from a file is drawn as a dot alone (CARD-131), because a picture can't read the file.
- **Heavy draws.** The reverb's first draw takes 3.4 ms and the Wavetable's up to 7.5 ms; one was stopped on CI's runners (CARD-75, CARD-110, CARD-131). Each needed "work it out once and keep it" code (`sumsWorked`, review T1) and a test that a build never runs it.

## 2. First principles

**What a picture needs from a sound.** Looking at every picture above, a picture needs one of four things:

| It needs | Pictures |
|---|---|
| how loud the sound is over time | the reverb's tail, the delay's echoes, the transient shaper's hit, Pump's dip, the tremolo, the envelope |
| one level for each steady level going in | the compressor's curve, drive's curve |
| the wave itself | the Drum's hit, a synth's note, a Wavetable frame, the crushed sine |
| how loud each frequency is | an EQ, a filter, the multiband's split, the ladder, FM partials |

The last two are what `p.hit()` and `p.response()` already give. The first two are the same render, read another way.

**Why TypeScript alone can't do it.** A picture's code runs in QuickJS on the server (CARD-64). The kernels run in the engine, in wasm sandboxes the renderer owns. Pack code has no way to play a kernel, and must not have one: it would be a renderer inside the sandbox, with a stranger's code driving it. So the general part must be in the core: play a copy of the call, and hand back a few numbers. That part exists already as the offline copy (CARD-81). This note makes it general instead of adding a third special layer.

**The rules it keeps.**
- It never touches what plays (RENDER-33, TAP-2): a copy in a renderer of its own, as today.
- A picture's code stays pure (CARD-64): the same values and the same measured numbers draw the same shapes.
- The kernels stay as they are: no kernel gains a "draw me" mode, no meter, no new bits.

## 3. The options

| | A. Keep the copies | B. Measure everything | **C. Measure what is the sound; values stay values** (recommended) |
|---|---|---|---|
| **What a pack writes** | its kernel's sums, again, in TypeScript | asks, and how to draw what comes back | asks for what the sound does; the dots, corners and guide lines from the values |
| **Right about the sound** | within 0.5 dB, at the settings tested, at 48 kHz | yes, at any rate | yes, at any rate, for everything but two small motion pictures (D7) |
| **Core grows by** | nothing | `measure`, plus "play from a later time" for snapshots of moving effects | `measure` (§4): one field, three inputs, three readings |
| **Pack lines** | about 900 lines of sums stay | about 950 go | about 900 go |
| **Tests** | a 0.5 dB render test for each picture, for ever | tests of the measure step once; each picture's test checks only its shapes | the same as B, and two small render tests stay |
| **CPU** | a draw of 1 to 4 ms | a measure of 1 to 27 ms, kept by values | the same as B |
| **A new sound** | its author copies its maths and writes a render test | its author asks for what to measure | the same as B |

B differs from C only in the chorus's and the phaser's motion: to measure them needs one more option (play the click from a later time, and read the sweep from several snapshots), and the pictures would change. C leaves them as they are for now and lets D7 decide.

## 4. The proposal: `measure`

### 4.1 What a pack writes

A picture's code (CARD-60) gains a third function beside `draw` and `words`: `measure`, a function of the card's values that names what to play and what to read. `draw` and `words` then get what was heard.

```ts
// @m2/effects: the reverb's tail, from its real sound.
tail: {
  measure: (v) => ({ tail: { step: -20, seconds: 12, levels: 512 } }),
  draw: (v, p, heard) => tail(p, v, heard.tail),          // std/pictures' backward sum, the −30 dB dot
  words: (v, heard) => tailWords(heard.tail),             // "Its tail fades 30 dB in 648 ms"
},

// std: the compressor's curve, 13 steady levels, its attack and release made fast for the measure.
transfer: {
  measure: (v) => ({
    out: { step: LEVELS, each: 0.02, set: { attack: 0.0001, release: 0.001 }, levels: LEVELS.length },
  }),
  draw: (v, p, heard) => transfer(p, v, LEVELS, heard.out, NAMES),
  words: (v) => transferWords(v, NAMES),
},
```

### 4.2 The words

One ask is a plain object: exactly one input, exactly one reading, and options. TypeScript types it by the sound's descriptor (as CARD-62 types `moves`), and the runner checks it again in words, as CARD-65 checks a draw.

| Kind | Word | Says |
|---|---|---|
| **input** (one) | `step: db` | a steady sound from the start, at that level (today's step; a level of −20 dB is today's click). A list of levels plays each in turn, each `each` seconds. |
| | `tone: { hz, db }` | a sine at that frequency and level |
| | `note: pitch` or `{ pitch, velocity }` | one note on the call's notes lane (today's hit) |
| **reading** (one) | `levels: n` | how loud it is in each of `n` equal slices of time, in dB (the mean power of each slice) |
| | `wave: n` | the wave in `n` columns: the highest and lowest sample of each (today's hit) |
| | `spectrum: [hz…]` | how loud each frequency is, at up to 512 frequencies. Of a `step`, it is the click's spectrum (today's response). Of a tone or a note, it is the sound's own spectrum over the time it was read, 2 s at most. |
| **options** | `seconds` | how long to listen, at most 12 s. Left out: until it has been quiet for 0.5 s (CARD-82's rule), up to 12 s. |
| | `from` | read from this many seconds in (a note after its attack) |
| | `held` | let the step or the note go after this many seconds (an envelope's gate, a note's length) |
| | `set: { name: value }` | play the copy with these controls set to these values instead. A list plays each in turn, with `each`. Never written. |
| | `each` | how long each step of a list lasts |
| | `left: true` | play the input into the left side only (what widen does to a stereo sound) |
| | `of: "side"` | read the side (left minus right) instead of the middle |

What comes back, for each ask by its name:

| Reading | `heard.<name>` |
|---|---|
| `levels` | `{ db: number[], each, heard }`: dB for each slice, how long a slice is, and how many seconds were heard |
| `wave` | `{ hi: number[], lo: number[], each, heard }` |
| `spectrum` | `{ db: number[] }`, one for each frequency asked |

`heard` is less than was asked only for a drag's frame that ran out of time (§7). A reading is at most 1,024 slices or columns. What it gives is data for `draw`, not shapes: the shapes `draw` makes from it count toward CARD-65's 20,000 numbers, as any shape does.

### 4.3 How it runs

1. The runner calls `measure(v)` in the picture's sandbox. It gives the asks, or none.
2. For each ask, the server makes the offline copy of the call (CARD-81) and plays it. What changes in the copy:
   - **Its input** is built from what a song already has, as today's step is: a note's key count scaled by std's `scale` (a step), std's `sine` (a tone), std's `pan` (left only), a control lane with stepped points (a list of levels or values), the notes lane (a note). No new kernel.
   - **It takes the input where it takes sound in**, or, for a call that takes no sound in, at its first control input (an envelope's gate).
   - **A control output can be read** (an envelope's, today refused as "it gives no sound").
   - **`set` changes a control of this call only,** inside its range; one a lane, a cable or the code drives can't be set ("can't set attack: a cable drives it").
3. Each ask is kept by what it plays (CARD-84's key, with the ask in it). The same ask at the same values is never played twice.
4. The runner calls `draw(v, p, heard)` and `words(v, heard)`, with `heard` frozen, as `v` is.
5. An ask that can't be measured (CARD-81's refusals, CARD-82's checks) or runs out of budget greys the picture with why, named by its card, as today.

**`p.response()` and `p.hit()` become std TypeScript.** std/pictures gets `curve(p, heard)` and `note(p, heard)`, each a few lines: a `step` read as a `spectrum` at 256 points across the axis, drawn as a line; a `note` read as a `wave` of 256 columns, drawn as a fill. The EQ, the filters, eq3, the Drum and the synths' notes move to them. The words `p.response` and `p.hit` leave the contract's list (CARD-61). `@m2/core` 2.4.0 has shipped in no release, so it changes in place, as CARD-100 did.

**CARD-82's check stays, for a click's spectrum only.** A `step` read as a `spectrum` is checked at rest at a 32nd of its level and from 0.1 s later, as today, and refused in words when it differs by more than 0.5 dB. A sound that changes with its level is read another way: steady levels, or a click quiet enough to sit under its thresholds (the multiband at −60 dB).

**Words may read what was heard** (D6). Today a picture's words in the view measure nothing (CARD-85). With this, the server measures after each accepted revision for the words too, on the pictures thread, kept by key, so only cards whose sound changed are measured again.

### 4.4 What stays drawn from the values

A dot, a guide line or a corner is a value, not a sound. These stay as they are:
- every dot (`p.handle`) and guide line (`p.mark`): the threshold, the crossovers, the cutoff, the echo time;
- an envelope's three corners, at its attack, decay and release;
- std's LFO shape (sine, triangle or saw: the name of the shape is the value) and the Wavetable's LFO sine;
- the gate's line, the de-esser's band (its edges from its Q of 1), the limiter's and the Master's ceiling;
- where things sit in time or pitch when that is a value: the delay's echoes every `time`, FM partials at `c + n·m` times the note. The measure gives how loud each is.

## 5. Picture by picture

Costs are CPU time on one thread at 48 kHz (96 kHz in brackets), from §6. "Drag" says whether a drag's frame fits in CARD-85's 8 ms.

| Picture | Ask | Cost | Drag | What is deleted |
|---|---|---|---|---|
| Reverb's tail | `step`, `levels: 512`, up to 12 s; std sums what is left from each slice (Schroeder's backward sum) | default room (size 0.5, damping 0.5) rings 1.2 s: about 4 ms. Size 1, damping 0, to 12 s: 26.5 ms (52) | default yes; size 1 shows the first 2.7 s and a fitted line (§7) | `room`, `tailLevel`, `tailFalls`, the comb tables; the render test |
| Delay's echoes | `step`, `levels` in 1 ms slices to the axis's 8 s; the echoes read at each `time` | 1.07 ms a second: 8.5 ms (17) at 8 s; about 2 ms for the default | mostly; the longest shows what fits | `echoLevels`; the render test |
| Compressor's curve | `step` of 13 levels, 20 ms each, attack and release `set` fast | 4.2 ms as 13 renders; about 1.5 ms as one stepped render (estimate) | yes | `turnedDown`, `KNEE`; the render test |
| Drive's curve | `tone: { hz: 20, db: 0 }`, `wave` of one cycle after 0.1 s; each sample out against the sine going in | under 1 ms (0.15 s of sound) | yes | `driven`; the render test |
| Bitcrusher | `tone: 1 kHz`, `wave` of 48 columns over 1 ms | under 1 ms | yes | `crushed`; the render test |
| Widen | a `step` into the left only, read `of: "side"`; a 3 kHz `tone`, read the side | under 1 ms each | yes | `madeUp`; two render tests |
| Transient shaper | `step` held, `levels` over 0.4 s: a sound that starts and holds, before (faint, the step) and after (D8) | 2.4 ms (4.3) | yes | `heard` and its closed-form sums; the render test |
| Multiband's split | `step: -60` (under its thresholds), `spectrum` at 256 points | 3.9 ms a drag frame, 6.0 ms at rest (8.1, 11.5) | yes | `splitDb`, `splitOf`; the render test |
| Pump's dip | `step`, `levels` over 4 beats from the song's first beat | 4.6 ms at 120 bpm (8.5); twice at 60 bpm | yes at 120 bpm | `dipDb`; the render test |
| Tremolo | `step`, `levels` over 1 s | 1.5 ms (2.7) | yes | `tremoloAt`; the render test |
| Envelope (std's `adsr`, and the synths' via their note) | `step` held for attack, decay and hold, into the gate; `levels` to the end of the release | 1.6 ms a second: about 1 ms for a pluck, 16 ms for 10 s | short ones yes; long ones partial | the 60 dB fall sums; the render test |
| Polysynth's ladder | two `note`s at C1, the filter's envelope and keytrack `set` off: one at its cutoff, one `set` wide open; the ratio of their `spectrum`s at the note's harmonics | about 3.9 ms (5.5): two notes 2.7 ms, two spectra 1.2 ms | yes (the wide-open one is kept) | `ladderDb`, `feedbackOf`; the render test |
| Wavetable's filter | the same, on the Wavetable | 18 ms the first time (two notes and their spectra); 9 ms a drag frame | one frame late (D9) | `svfDb`; the render test |
| FM partials | a `note`, pair A alone and pair B alone (`set` the mix), `spectrum` at each partial's place | about 1.7 ms (2.0): two notes 1.3 ms, two short spectra about 0.4 ms | yes | `bessel`, `indexAt`, `partialsOf`; the render test |
| Wavetable's frames | a `note` at a pitch whose cycle is whole samples, `set` the filter open and table B silent; `wave` of one cycle. The eight frames behind: one render, positions `set` as a list | 8.5 ms a render; nine separate renders would be 77.8 ms | the frames behind are kept; the one playing is 8.5 ms (D9) | `harmonics`, `frameAt`, the vowels; the render test. A table from a file is drawn too. |
| EQ, filters, eq3 (today `p.response`) | `step`, `spectrum` | as today: 3.3 ms for the EQ (CARD-85); 11.3 ms with a band at 40 Hz, q 18, which rings long | as today | `p.response` from the core |
| Drum, synth notes (today `p.hit`) | `note`, `wave` | as today: 2.6 ms for a 1 s kick (4.7) | yes | `p.hit` from the core |
| Chorus, phaser | keep their small sums (D7) | | | |
| std's LFO | stays drawn from its values (§4.4) | measuring would cost 1.3 ms a second: 21 ms for 16 beats at 60 bpm | | |

## 6. The costs, measured

**How.** A scratch copy of `main` at `c653658c` with one test file (`$SCRATCH/measure2-note/proto_tests.rs`) and a five-line helper in `measure.rs` that renders a copy's step at any level. Each number is the CPU time of the test's thread (`m2_os::thread::cpu_time`, as CARD-85 counts a measure), the median of 7 runs, in the dev build the nightly timing tests use. The shared Linux server (32 cores) at a load under 1, run alone.

**Songs, rendered from sample 0** (Renderer setup, then the render):

| Sound | Setup | Render | A second of it | at 96 kHz, a second |
|---|---|---|---|---|
| reverb, size 1, damping 0 | 0.21 ms | 28.0 ms for 12 s | 2.33 ms | 4.67 ms |
| reverb, size 0.5, damping 0.5 | 0.21 ms | 9.1 ms for 4 s | 2.28 ms | 4.43 ms |
| delay, 1 s, feedback 0.9 | 0.28 ms | 8.5 ms for 8 s | 1.07 ms | 2.14 ms |
| std's compress | 0.26 ms | 3.9 ms for 1 s | 3.92 ms | 7.93 ms |
| drive | 0.18 ms | 0.2 ms for 0.05 s | 3.91 ms | 7.36 ms |
| widen, a sound panned left | 0.26 ms | 0.17 ms for 0.1 s | 1.75 ms | 3.32 ms |
| transient shaper on a hit | 0.44 ms | 2.0 ms for 0.4 s | 5.03 ms | 9.75 ms |
| multiband | 0.20 ms | 8.4 ms for 1 s | 8.44 ms | 16.6 ms |
| Pump, a bar at 120 bpm | 0.62 ms | 4.0 ms for 2 s | 1.98 ms | 3.96 ms |
| chorus | 0.18 ms | 3.0 ms for 1 s | 3.02 ms | 5.69 ms |
| phaser | 0.17 ms | 2.7 ms for 1 s | 2.72 ms | 5.54 ms |
| tremolo | 0.18 ms | 1.3 ms for 1 s | 1.28 ms | 2.51 ms |
| adsr (through a vca) | 0.49 ms | 15.8 ms for 10 s | 1.58 ms | 3.04 ms |
| std's lfo (through a vca) | 0.28 ms | 20.8 ms for 16 s | 1.30 ms | 2.63 ms |
| the eight-band EQ | 0.20 ms | 5.0 ms for 1 s | 5.02 ms | 9.96 ms |

**Copies, as a measure makes them** (`copy`, then its render, end to end):

| Measure | 48 kHz | 96 kHz |
|---|---|---|
| making the copy itself (the reverb's) | 0.04 ms | 0.06 ms |
| the reverb's click, 12 s, size 1, damping 0 | 26.5 ms; its energy falls 30 dB by 5.58 s and 60 dB by 11.10 s | 52.0 ms; 5.57 s and 11.08 s |
| the reverb's click, 12 s, size 0.5, damping 0.5 | 26.2 ms (it fell 60 dB by 1.17 s, so a measure that stops when quiet listens about 1.7 s: about 4 ms) | 52.1 ms |
| the compressor, 13 levels, 0.1 s each, at its default attack (10 ms) | 6.7 ms | 9.8 ms |
| the same, at an attack of 1 s, 0.1 s each | 6.6 ms, **and the wrong curve**: −1.3 dB out at 0 dB in, where the settled curve gives −13.4 | |
| the same, at an attack of 1 s, 5 s each | 161.6 ms: right, but far too slow | |
| the same, attack and release `set` to 0.1 ms and 1 ms, 0.02 s each | **4.2 ms, the settled curve within 0.1 dB** | |
| the multiband's click: a drag frame, at rest | 3.9 ms, 6.0 ms | 8.1 ms, 11.5 ms |
| the EQ's click with a band at 40 Hz, q 18: a drag frame, at rest | 11.3 ms, 13.1 ms | 18.8 ms, 21.9 ms |
| a polysynth note of 0.3 s; one of 0.1 s wide open | 1.75 ms; 0.91 ms | 2.97 ms; 1.30 ms |
| an FM note of 0.05 s | 0.65 ms | 0.81 ms |
| a Wavetable note of 0.01, 0.03, 0.3, 1 s | 8.55, 8.45, 9.58, 12.77 ms | 8.60 ms (0.03 s) |
| nine Wavetable notes of 0.03 s, each its own render | 77.8 ms | 77.3 ms |
| a Drum hit of 1 s | 2.6 ms | 4.7 ms |
| a spectrum at 256 points of 2,048, 8,192, 16,384, 32,768 samples (today's `response`, which a `spectrum` reading is) | 0.17, 0.58, 1.15, 1.35 ms | |
| the same of 576,000 samples (12 s) | 60 ms: why a spectrum reads 2 s at most | |

**What the numbers say.**
- **Playing a copy is cheap: 1 to 8 ms a second of sound at 48 kHz,** twice that at 96 kHz. Setting up a copy costs 0.2 to 0.6 ms.
- **Only long sounds cost much.** The longest reverb is 26.5 ms. Every other picture's measure is under 10 ms, most under 5.
- **The Wavetable costs 8.4 ms before it plays a sample.** Its kernel makes its table (32 frames, 9 mip levels, by inverse FFTs) at its first voice's birth, in each sandbox (`shelf.rs`). A copy has sandboxes of its own, so every measure makes the table again. One render for all its frames costs about the same as one frame; nine renders cost nine times as much. Hence the stepped `set` (§4.2), and the later option of keeping a copy's sandboxes warm (D9).
- **A level for each steady input needs `set`.** A compressor whose attack is 1 s needs 5 s at each level to settle. Read too early it draws a wrong curve (−1.3 dB in place of −13.4). Setting its attack and release fast for the measure draws the settled curve in 4.2 ms. A compressor's settled curve doesn't depend on its attack and release; its test (§11, track `measure2`) checks that by measuring both ways.
- **Reading is cheap if done by slices.** My first backward sum took a log of every sample: 7.1 ms over 12 s. Done on 512 slices, as `levels` gives them, it is a sum of the samples and 512 logs (not timed; far under 1 ms).
- **For comparison,** today's reverb picture takes 3.4 ms on its first draw and 1.4 ms after. Measuring costs more CPU for the biggest rooms. It is kept by values, runs beside the draw rather than in it, and is right.

## 7. Live drags

A drag's frame must come back drawn within one screen frame (CARD-75), and CARD-85 gives a measure 8 ms of a drag's frame at 48 kHz.

1. **Kept by what it plays.** Each ask's key is its copy (CARD-84) and the ask. A value the ask doesn't play changes no key: dragging a compressor's attack measures nothing, since its curve is measured with attack `set`; dragging an EQ band that is off measures nothing. A drag that comes back to where it was measures nothing.
2. **The asks that don't move are kept.** The Wavetable's frames behind the one playing, and a filter's wide-open reference note, don't depend on the dragged value, so they are measured once.
3. **A long sound draws what it heard so far.** In a drag's frame, a picture's asks share 6 ms of CPU. An ask that runs out stops and gives what it heard, with `heard` saying how many seconds. The picture draws that, and std's `extend` may draw the rest faint, as a straight line fitted to the last second heard (the reverb at size 1: its first 2.7 s). That line is statistics on the measured sound, not a copy of the kernel. When the drag ends, it is measured whole.
4. **Coarse by length, never by rate.** A kernel at another sample rate is another sound, so a quick measure listens less; it never renders at a lower rate.
5. **What is still over a frame:** a Wavetable note (8.5 ms, the table's making). Its frame follows a position drag one screen frame late. Keeping a copy's sandboxes warm between measures would make it under 1 ms (D9); that is an engine change, left for later.

At rest (no drag), a picture's asks share a budget of **40 ms of CPU at 48 kHz** (80 ms at 96 kHz), in place of today's 20 ms for one measure (D5). The longest reverb, 26.5 ms, fits. Past it, the picture greys and says so, and is tried again in 10 s, as CARD-85 says today.

## 8. Sample rate

A copy plays at the song's rate (CARD-81), so every measured picture is right at 44.1, 48 and 96 kHz with no extra code. Today's sums that assume 48 kHz all go: the effects' `RATE`, the transient shaper's and the split's, the FM partials' index limit "at 48 kHz", and the bitcrusher's 48 samples. A measure at 96 kHz costs twice the CPU, and its budget is twice as big, as CARD-85 already says.

## 9. Risks

| Risk | What happens | Answer |
|---|---|---|
| **A sound that isn't the same each time** (noise, a random detune) | Kernels are deterministic: a render is the same bits every time (RENDER-33), and noise is seeded. A copy always plays from sample 0. | The picture shows one take of it, the same each time. CARD-84's keeping already relies on this. |
| **A very long tail** (a reverb at size 1, a delay with feedback 0.95) | It is cut at 12 s, the longest axis a picture has. The backward sum then reads a little low near its end: under 2 dB at the −60 dB point of the biggest room (estimate), nothing at its −30 dB dot. | Listen to the axis's end; say in the words when it was cut; fit the rest faint (§7). |
| **A value driven by a cable or a lane** | As CARD-81: a `levels` or `wave` ask plays the work as it is copied (an LFO on the delay's time shows smeared echoes: the truth); a click's `spectrum` and a list of steady levels are refused ("it has no one curve"); `set` can't change it. | Kept as today, said in words. |
| **A sound that listens to another track** (a compressor keyed by the kick) | Its sidechain hears silence (CARD-81), so a compressor would never turn down. | D10: for steady levels, the sidechain hears the same steady sound, and the words say so. |
| **A plugin** | Refused, as today. | Unchanged. |
| **A filter inside a voice** | A click can't reach it (CARD-81), so it is measured from notes: the ratio of a note at its cutoff to the same note wide open, at the note's harmonics. A low note (C1, 32.7 Hz apart) gives points across the whole axis. | The polysynth's and the Wavetable's tests check it against today's drawn curves. |
| **`set` changes what you measure** | A picture could set values so the picture lies. | Only this call's controls, in range, never written; the words say what was set ("measured with the filter open"). |
| **A slow kernel** (a pack's) | CARD-85's limit for each call into a pack's kernel and the whole budget, by CPU time. | Unchanged. |
| **More measuring when a song opens** (D6) | Each picture's words measure once. 20 cards at about 5 ms is 100 ms on the pictures thread, once. | Kept by key: a change measures again only the cards it changed. |
| **The draw needs the measure first** | A picture appears after its measure: 1 to 27 ms later. | Today's measured layers already wait for their measure; the frame shows meanwhile (CARD-69). |

## 10. What the core gains and loses, counted

**Gains.**

| Layer | Gains | Count |
|---|---|---|
| SDK (`@m2/core` 2.4.0, unshipped, changed in place) | a picture code's `measure`; an ask's inputs `step`, `tone`, `note`; its readings `levels`, `wave`, `spectrum`; its options `seconds`, `from`, `held`, `set`, `each`, `left`, `of`; the types of `heard`; the checks | **1 field, 6 words, 7 options**; about 150 lines |
| The picture runner | `measure(v)` before `draw`, then `draw(v, p, heard)` and `words(v, heard)` | one more call into the sandbox for a picture that measures; about 60 lines |
| The offline copy (`crates/editor/src/measure.rs`) | inputs built from std's `sine` and `pan` and a stepped control lane; a step into a control input; a control output read; `set`; the three readings | about 250 lines |
| Server (`crates/server/src/graph/pictures.rs`) | asks in place of shapes to fill; a picture's budget; a drag's share and a partial answer; words measured after a revision | about 80 lines |
| | | **about 540 lines in all** |
| Kernels, routes, descriptors, the view, the window, the engine | nothing | **0** |

**Loses.**

| Layer | Loses | Count |
|---|---|---|
| SDK | `p.response`, `p.hit` and their checks (now std TypeScript over `measure`) | **2 words**; about 60 lines |
| Packs and std | the copied sums of §1 but the chorus's and the phaser's; std's drawn lowpass (CARD-97, used by no shipped sound); the "work it out once" code (`sumsWorked`) | **about 900 lines** (counted by section, rounded); about 150 lines of std helpers come back (`curve`, `note`, a backward sum, `extend`, reading a level per step) |
| Tests | the render tests of copied sums: 3 in `std_pictures_tests.rs`, 10 in `pack_pictures_tests.rs`, 4 in `instrument_pictures_tests.rs`, and `a_build_works_out_none_of_the_pictures_sums` | **about 700 lines**; about 300 lines of tests of the measure step come in (§11) |
| Contract | CARD-97, CARD-125 (but chorus and phaser), CARD-132 and CARD-137 go; CARD-99 shrinks to the LFO; "from its own sums" becomes "measured" in CARD-93, 96, 113, 115 to 118, 121 to 124, 131, 133, 136 and 140 | **4 rules gone, about 16 shorter**; CARD-80, 81, 83 and 85 rewritten, CARD-87 to CARD-89 added (the ids are the lead's to give) |

Net: the core grows by about 480 lines (540 in, 60 out) and no kernel; the packs and std shrink by about 750 lines; the tests by about 400.

## 11. Migration

Each track as in [Typed sounds](../cards/design.md) §11: a builder, a skeptic, the builder again. Test first: each picture's new test fails on today's sums before its sums go.

| # | Track | Builds | Size |
|---|---|---|---|
| 1 | `measure2` | The contract first (CARD-80 to CARD-89). `measure`, its asks and `heard` in `@m2/core`, typed and checked; the runner's two calls; the copy's inputs, `set`, control outputs and readings; budgets, the drag's share and partial answers; words that read what was heard. `p.response` and `p.hit` become std's `curve` and `note`, and the EQ, filters, eq3, Drum and synth notes move to them (the proof: their pictures don't change). Tests of each input and reading against a known sound (a step through std's lowpass, a tone through drive, a compressor's settled curve both ways). | medium to large (about track `measure`) |
| 2 | `measure2-fx` | `@m2/effects`: the reverb, delay, drive, bitcrusher, tremolo and widen measured; their sums and render tests go; a minor version | medium |
| 3 | `measure2-mix` | `@m2/mixing`: the transient shaper (D8), the multiband, Pump; std: the compressor's curve, the envelope; std's drawn lowpass goes | medium |
| 4 | `measure2-inst` | `@m2/synths`: the ladder and the FM partials; `@m2/wavetable`: its frames (a file's table drawn too) and its filter | medium |
| 5 | `warm` (only if D9 says so) | keep a copy's sandboxes between measures of the same call, so the Wavetable's table is made once | small to medium, in the runtime |

Track 1 first. Tracks 2, 3 and 4 then run side by side: each touches one pack, and std only in 3.

## 12. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | Should a picture show what the sound really does, measured by playing it, wherever it can, so the packs' copies of their sounds' maths are deleted? | **Yes.** One truth instead of two, right at every sample rate, and about 900 fewer lines. |
| D2 | Should that be one new tool for packs (`measure`: play a step, a tone or a note in; read the levels, the wave or the frequencies out) that also replaces the two special ones we have now (`p.response`, `p.hit`)? | **Yes.** One general tool instead of two special ones and many copies. |
| D3 | While you drag, a big reverb takes longer than a screen frame to measure. Should the picture show the part it has heard, with the rest as a faint straight line, and measure the whole when you let go? | **Yes.** The curve keeps up with your hand and is right when you stop. |
| D4 | May a picture measure its sound with a few knobs set differently, never written (a compressor's attack made fast, a synth's filter opened), to see one thing at a time? | **Yes.** Without it a slow compressor needs 5 s a level to measure, or draws the wrong curve. The words say what was set. |
| D5 | May a picture's measuring use up to 40 ms of computer time at rest (80 at 96 kHz), up from 20, so the biggest reverb is measured whole? | **Yes.** It runs away from the audio and is kept, so it costs once for each new setting. |
| D6 | May a picture's one-line words use what was measured ("Its tail fades 30 dB in 648 ms")? Each changed card is then measured after each change to the song. | **Yes.** It is the sentence an agent and a folded card most need; it costs a few ms for each changed card. |
| D7 | The chorus's and the phaser's moving pictures keep a small copy of their maths for now. Measure them too, later, with one more option (play from a later moment)? | **Keep them for now.** They are small and tested; measuring them changes what they show. |
| D8 | The transient shaper's picture would show a sound that starts and holds (what is easy to play in), before and after, instead of a drum-like hit that dies away. Is that all right? | **Yes.** It shows both of its knobs (the start and the hold) from the real sound. |
| D9 | A Wavetable picture would follow a drag about one screen frame late (8.5 ms), because its kernel makes its table each time it is measured. Fix it later by keeping the measuring copies warm, only if it bothers you? | **Later, if it bothers you.** It is an engine change; the picture is right either way. |
| D10 | A compressor that listens to another track: should its curve be measured as if it listened to itself, and say so? | **Yes.** Measured with silence on what it listens to, it would never turn down. |
| D11 | Should `m2 pack check` measure each picture in a one-track song it makes, so a pack's author sees a broken measure before publishing? | **Yes.** Today it measures nothing (CARD-80). |
