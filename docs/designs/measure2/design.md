# Pictures measure the real sound

| | |
|---|---|
| **Status** | Proposed, for the owner's yes (AGENTS.md rule 11). Nothing is built. Revised after the skeptic's review (M2-1 to M2-21, §12). |
| **Written** | 2026-11-07, from `main` at `c653658c`, track `measure2-note` |
| **Builds on** | [Cards you play with](../cards2/design.md) §4.7 and D13 ("measure where you can"), and the contract's [Curves measured from the sound](../../architecture.md#curves-measured-from-the-sound) (CARD-80 to CARD-86) |
| **About** | the pictures of std, `@m2/mixing`, `@m2/effects`, `@m2/synths` and `@m2/wavetable` that draw from sums copied out of their kernels (CARD-93, CARD-96 to CARD-98, CARD-113, CARD-115 to CARD-124, CARD-131 to CARD-137, CARD-140) |
| **Prototypes** | `$SCRATCH/measure2-note/proto_tests.rs` (the first costs) and `proto2_tests.rs` (p99 costs, a windowed spectrum, the multiband's quiet click), each over `measure-raw.patch`; the skeptic's `review_step_levels` in `$SCRATCH/measure2-note/review/` |

**In short**
- **Today about fifteen pictures copy their sound's maths into TypeScript.** Each is a second copy of the truth, kept honest by a test that renders the sound and checks the picture within 0.5 dB. About 900 lines of copied sums, and about 700 lines of tests that guard them.
- **I looked for one general tool, `measure`,** that plays a copy of the sound and hands numbers to the picture. The first version of this note proposed it for every picture. The review showed it would draw some pictures wrong, make the Wavetable worse while you drag, and grow the core by 14 names.
- **Narrowed honestly, it is worth measuring for four pictures:** the reverb's tail, the transient shaper's hit, the multiband's split and the FM partials. Everything else is either exact and cheap as a sum (the compressor, drive, Pump, the tremolo, the delay, widen, the bitcrusher, the envelopes, the LFOs, chorus, phaser) or can't be measured cheaply yet (the Wavetable, whose kernel spends 8 to 10 ms making its table each time).
- **For those four, the tool costs more core than it saves.** It needs 8 names and about 550 core lines (and about 250 core test lines), to delete about 450 net pack lines. Counted the RISC way, with a core line worth about three pack lines, that is a loss. Even counted one for one, the total grows. The big reverb would also redraw slower than today while you drag its size.
- **So the recommendation is: don't build `measure` now. Keep the copies, with their tests,** and take three small steps instead:
  1. the multiband's split is measured with today's `p.response()`, given one new option, a quiet `level` (a click at −80 dB sits under all its thresholds; prototyped, §5);
  2. std's old drawn lowpass, which no sound uses, goes;
  3. every copy's render test also runs at 44.1 and 96 kHz, so a copy that is right only at 48 kHz is found.
- **The narrowed `measure` is written down (§4), ready for later,** with its costs at p99 on a base M1 (estimated, §5). Build it when an outside pack needs it, when a copy breaks for real, or when the Wavetable can be measured cheaply.

## 1. What is wrong today

The cards2 design (§4.7, D13) said: a curve that can be measured is measured, and the pack's own maths is for what a click can't measure. A click (`p.response()`) can measure only a sound that stays the same at every level and every time, and that rings for under 2 s. Everything else is drawn from sums the pack copied out of its kernel:

| Picture | Rule | What the pack copies | Its test against a render |
|---|---|---|---|
| Reverb's tail | CARD-118 | Freeverb's eight combs and four all-passes, their lengths at 48 kHz, 32 frequency bands, how the combs cancel in the all-passes (about 240 lines) | `the_reverbs_tail_is_its_sound` |
| Transient shaper's hit | CARD-113 | its followers, its 25 ms window, its glide, each share, summed in closed form (about 180 lines) | `the_transient_shapers_hit_is_its_sound` |
| Wavetable's frames | CARD-131, CARD-132 | the five built-in tables' harmonics, the vowels' formants, the kernel's mixing of frames (about 170 lines) | `the_drawn_tables_are_the_wavetables_sound` |
| FM synth's partials | CARD-140 | Bessel functions, the index limit at 48 kHz, the tone's brightness, fold-back (about 80 lines) | `the_drawn_partials_are_the_fm_synths_sound` |
| Multiband's split | CARD-115 | Linkwitz-Riley crossovers and the all-pass (about 45 lines) | `the_multiband_split_is_its_sound` |
| Polysynth's ladder, Wavetable's filter | CARD-133, CARD-136 | the ladder's feedback curve, four state-variable shapes (about 55 lines) | two tests |
| std's drawn lowpass | CARD-97 | a two-pole lowpass (about 45 lines); no shipped sound uses it now | one test |
| Drive's curve, bitcrusher's sine | CARD-122, CARD-123 | `tanh`, the crusher's clock and rounding (about 40 lines) | two tests |
| Compressor's curve | CARD-93 | the knee, the ratio, the makeup (about 25 lines) | one test |
| Envelopes | CARD-96 | adsr's 60 dB falls (about 25 lines) | one test |
| Tremolo, delay, Pump, widen | CARD-121, 117, 116, 124 | a cosine, a feedback power, the dip's power, the made-up width (about 35 lines) | five tests |
| LFOs, chorus and phaser motion | CARD-98, 134, 119, 120 | the LFO shapes; 8 ms and 6 ms; 640 Hz and 2.5 octaves (about 40 lines) | three tests |

What that costs:
- **Two truths.** Change a kernel and its picture lies until someone changes the TypeScript too. The 0.5 dB tests catch it, but only at the settings each test tries, and only at 48 kHz.
- **48 kHz only.** Several sums are worked out at 48 kHz (CARD-110, CARD-140). A song at 96 kHz may be drawn a 48 kHz sound. No test checks it today.
- **What can't be copied isn't drawn.** A Wavetable table loaded from a file is drawn as a dot alone (CARD-131).
- **Heavy draws.** The reverb's first draw takes 3.4 ms and the Wavetable's up to 7.5 ms (CARD-110, CARD-131), each with "work it out once and keep it" code.

## 2. First principles

**What a picture needs from a sound.** Every picture above needs one of these, and each needs a different thing played in:

| It needs | Played in | Read | Pictures |
|---|---|---|---|
| what a click becomes over time | a **click** (a step whose first difference is read) | levels over time | the reverb's tail, the delay's echoes |
| what a held sound becomes over time | a **steady sound** (or a list of levels, as a shape) | levels over time | the transient shaper's hit, Pump's dip, the tremolo |
| one level for each steady level going in | steady sounds at several levels | the level once settled | the compressor's curve, drive's curve |
| the wave itself | a **note** | the wave | the Drum's hit, a synth's note, a Wavetable frame |
| how loud each frequency is | a click, or a note | a spectrum | an EQ, a filter, the multiband's split, FM partials |

A click and a steady sound are both a step to the engine. What differs is what is read: the click is the step's first difference, the steady sound is the step itself. The first draft gave them one word, so the reverb would have been drawn flat (M2-1). They need two.

**Why TypeScript alone can't measure.** A picture's code runs in QuickJS on the server (CARD-64). The kernels run in the engine, in wasm sandboxes the renderer owns. Pack code can't play a kernel, and must not: it would be a renderer inside the sandbox, driven by a stranger's code. So measuring needs the core. Today's core already has two special cases of it, `p.response()` and `p.hit()`, filled in by music2 from an offline copy (CARD-81).

**What a measure must not do.**
- Touch what plays (RENDER-33, TAP-2): it is a copy, in a renderer of its own.
- Change a kernel: no kernel gains a "draw me" mode.
- Draw a different sound from the one on the card, or say a sum is the sound when it is only the kernel's target (M2-8).
- Make the UI slower than today's drawing for the gesture the picture is for (M2-3).

## 3. Which pictures are worth measuring

Costs are p99 CPU at 48 kHz on a base M1, estimated from this server (§5).

| Picture | Can it be measured exactly? | Measure's cost (est. base M1, p99) | Today's sum | Verdict |
|---|---|---|---|---|
| **Reverb's tail** | yes: a click, levels over time, from 1 ms on so the dry sound is left out (M2-2) | default room 7.7 ms; size 1 at 12 s, 43 ms | about 240 lines; first draw 3.4 ms | **worth measuring**, but a big room redraws slower while you drag |
| **Transient shaper's hit** | yes: a list of levels shaped as today's hit (up in 1 ms, down 60 dB in 0.3 s), 0.4 s | 3.8 ms | about 180 lines of closed-form sums | **worth measuring** |
| **Multiband's split** | yes: a click at −80 dB, under every threshold (−60 dB lowest; prototyped) | 5.8 ms dragging, 10.1 ms at rest | about 45 lines | **worth measuring, with today's `p.response`** given a `level` |
| **FM partials** | yes: a note, a windowed spectrum (Blackman-Harris) at each partial's place, at least 4 ÷ the closest spacing long (M2-11) | 3.6 ms | about 80 lines, with a 48 kHz index limit | **worth measuring** |
| Wavetable's frames, filter, envelope | the kernel spends 8 to 10 ms making its table in every copy, before a sample; frames also need unison off (a setting) and 70 ms per position (M2-9) | 11 to 13 ms a drag frame, over a frame | about 225 lines, fast | **stays a sum** until the copy's table can be kept (`warm`, D5) |
| Compressor's curve | only as its static target, which is exactly the 25-line sum (M2-8) | 4.2 ms where the sum is microseconds | 25 lines | **stays a sum**; its words call it "the curve it aims for" |
| Envelopes | std's adsr only; a synth's is measured through its note, which isn't the corners' curve (M2-19); long ones pass the 12 s cap | 1 to 20 ms | 25 lines | **stays a sum** |
| Ladder, std's drawn lowpass | the ladder needs a long list of values set to read (M2-8) | | 30 and 45 lines | ladder **stays a sum**; std's lowpass **goes** (unused) |
| Drive, bitcrusher, widen, Pump, tremolo, delay | yes, but each sum is a few lines and exact; the delay needs each slice's loudest sample (M2-14), widen two stereo words (M2-4) | 1 to 9 ms | 5 to 30 lines each | **stay sums** (cheap and exact) |
| LFOs, chorus, phaser | the LFO's shape is its value; chorus and phaser would need snapshots and new pictures | | about 40 lines | **stay sums** |

**Copies that stay, whatever is chosen** (M2-20): std's LFO shape and phase, the Wavetable's LFO sine, the chorus's and the phaser's motion, the de-esser's band edges (its Q of 1), the places of FM partials (c + n·m, folded back), the compressor's target curve, drive's `tanh`, the delay's echo levels, Pump's dip, the tremolo, widen's made-up width, the bitcrusher's steps, the envelopes, the ladder and the Wavetable's tables and filter. Each keeps its render test (§6.2 makes those tests run at three rates).

## 4. The narrowed `measure`, for later

This is the smallest tool that measures the four pictures of §3. It starts from the skeptic's set (M2-4) and drops what no picture in §3 needs.

### 4.1 What a pack would write

A picture's code (CARD-60) gains a third function beside `draw` and `words`: `measure`, a function of the card's values that names what to play and what to read. `draw` gets what was heard. `words` does not (D4).

```ts
// @m2/effects: the reverb's tail. A click, read from 1 ms on so the dry sound is left out (M2-2).
tail: {
  measure: (v) => ({ tail: { click: -20, columns: 512, from: 0.001 } }),   // until quiet, 12 s at most
  draw: (v, p, heard) => tail(p, v, heard.tail),   // std/pictures: what is left, summed backward; the −30 dB dot
  words: (v) => tailWords(v),                      // "A big room (size 0.9), damped 0.5. Drag the dot for the size"
},

// @m2/mixing: the transient shaper. Today's hit, as a list of levels, 1 ms each.
hit: {
  measure: (v) => ({ out: { step: { levels: HIT, each: 0.001 }, columns: 400 } }),
  draw: (v, p, heard) => hit(p, v, HIT, heard.out),
  words: (v) => hitWords(v),
},
```

### 4.2 The words

One ask: exactly one input, exactly one reading, and up to two options. TypeScript types it, and the runner checks it again in Rust, in words, as CARD-65 checks a draw.

| Kind | Word | Says |
|---|---|---|
| **input** (one) | `click: db` | a step at that level whose first difference is read: what a click becomes. It listens until quiet (0.5 s under −90 dB of its loudest), 12 s at most. Today's `p.response` click is `click: -20`. |
| | `step: db` or `step: { levels, each }` | a steady sound read as it is; a list plays each level in turn, `each` seconds, through a stepped control lane in the copy. Its length is the list's (or `seconds`); it never "listens until quiet", since a steady sound never falls quiet (M2-1). |
| | `note: pitch` | one note on the call's notes lane, held for `seconds` (today's hit) |
| **reading** (one) | `columns: n` | `n` equal columns of time (1,024 at most), each with its highest and lowest sample and its mean power. The wave (`hi`, `lo`) and the level (`power`) are one reading (M2-4). |
| | `spectrum: [hz…]` | how loud each frequency is, 512 at most. Of a click, the click's spectrum (today's response, with CARD-82's checks at rest). Of a step or a note, the sound's own spectrum through a Blackman-Harris window, refused when the read is shorter than 4 ÷ the closest spacing asked ("too short to tell 30 Hz from 40 Hz", M2-11); 2 s at most. |
| **options** | `seconds` | how long to listen (a note, a steady sound); 12 s at most |
| | `from` | read from this many seconds in |

That is **8 names: the field `measure`, 3 inputs, 2 readings, 2 options** (10 counting `levels` and `each` inside a list). `p.response` and `p.hit` become std TypeScript over them (`click` with `spectrum`; `note` with `columns`), so the core loses those **2 words**.

**Left out, and why** (M2-4, M2-8, M2-10):
- `tone`, `left`, `of: "side"`: only drive, the bitcrusher and widen needed them, and their sums stay.
- `set`: every ask in §3 plays the card's own values. When a picture needs a value held for the measure, it comes as `only: [names]` (every other control at its default, the words saying so), never as a list of values to set. That form survives a kernel's new control and keeps the keys stable.
- `held`: a note is held for `seconds`.
- A reading in samples: no picture in §3 reads a whole number of samples, so no ask depends on the rate.

### 4.3 How it would run

1. The runner calls `measure(v)` in the picture's sandbox and checks the asks.
2. Each ask is played in the call's offline copy (CARD-81), kept by the copy's key and the ask (CARD-84). The cache holds 256, measures at rest apart from drag frames' (M2-6). A measure in progress is shared between the pictures thread and an ask's pool thread, never made twice (M2-6).
3. The runner calls `draw(v, p, heard)`, `heard` frozen. Both of CARD-64's first runs get the same `heard`.
4. An ask that can't be measured, or runs out of budget, greys the picture with why, as today.

**While you drag.** There are no partial answers and no guessed lines (M2-7). A picture whose measure fits a frame follows every frame. One that doesn't is redrawn as fast as its measure allows, at most 30 times a second, from the newest values; the dot stays under the pointer, as it does today (CARD-70), and the rest follows. At 48 kHz on a base M1 (est.) the default room, the transient shaper, the multiband and the FM partials fit; a room at size 1 redraws about 20 times a second. At 96 kHz the default room and the multiband (12 to 13 ms each) no longer fit a frame and redraw 30 times a second.

**Budget.** A picture's asks get at most 60 ms of CPU at 48 kHz (120 at 96 kHz), up from today's 20 for one measure, so the size-1 reverb (est. 43 ms p99 on a base M1) fits with about 30% to spare. Past it, the picture greys, says the computer may be busy, and is tried again in 10 s (CARD-85). That is what a slow or busy laptop gets for a huge room.

**Thread and priority** (M2-18). Measures run on the `music2-pictures` thread at normal priority, as draws do. Not background: on a Mac that moves them to the slow cores and doubles their cost. The audio thread and the live pool run real time (RENDER-28a), so they come first; but a measure shares their cache. So: at most 30 measures a second for each picture, one in flight, and track 1b must run gate 8's p50 dropout check with a reverb's size dragged for a minute on the reference Mac before it lands.

**Words read no measure** (D4). A picture's words in the view stay a function of its values (CARD-85: words measure nothing). That keeps M2-6's problems away: no measuring of every card at each revision, no words kept stale by a key without the tempo. The cost is that the reverb's words say its size, not how long it rings; the picture shows that.

**`m2 pack check`** (old D11). It plays no song today. Measuring there means a one-track song in the CLI: about 100 more core lines. Left out of the narrowed plan; a broken ask is still checked in words, unplayed.

### 4.4 What stays drawn from values

Every dot and guide line, and the places of things that are values: the delay's time, FM partials at c + n·m times the note. The measure gives only how loud each is.

## 5. Costs, measured

**How.** A scratch copy of `main` at `c653658c` with `measure-raw.patch` (a five-line helper that renders a copy's step at any level). CPU time of the test's thread (`m2_os::thread::cpu_time`, as CARD-85 counts), dev build, as the nightly timing tests use. The first table: medians of 7 runs, load under 1. The p99 table: 100 runs each, while the load average was 8 to 19 (other tracks building), so its p99s include a busy machine.

**From this server to a base M1** (M2-5). CARD-85 says a budget holds at p99 on the slowest computer music2 supports, a base M1 (HANDOFF.md: provisional). This server is a Ryzen 9 7950X3D. Published single-thread benchmarks put it about 1.2 to 1.3 times a base M1 (Geekbench 6 single-core: about 2,900 against about 2,300). I scale this server's p99 by **1.3** and call the results estimates. They need a run on the owner's Mac (or `--like-m1`, once it exists) before any budget is set from them.

**p99, the narrowed plan's measures** (`proto2_tests.rs`, `proto_p99`):

| Measure | 48 kHz: p50, p99 here | p99, base M1 (×1.3, est.) | 96 kHz: p50, p99 here | p99, base M1 (est.) |
|---|---|---|---|---|
| Reverb at size 1, damping 0: click, 12 s, 512 columns, backward sum | 28.7, 33.2 ms | 43 ms | 56.7, 63.8 ms | 83 ms |
| Reverb at its defaults: click until quiet (1.86 s) | 4.7, 5.9 ms | 7.7 ms | 9.1, 9.3 ms | 12.1 ms |
| Multiband: click spectrum, a drag frame | 4.1, 4.5 ms | 5.8 ms | 8.6, 9.9 ms | 12.9 ms |
| Multiband: at rest, with CARD-82's checks | 6.1, 7.8 ms | 10.1 ms | 12.2, 12.9 ms | 16.7 ms |
| EQ (today's `p.response`), a drag frame / at rest | 3.4, 3.8 / 5.6, 6.4 ms | 4.9 / 8.4 ms | 6.9, 7.2 / 10.8, 12.7 ms | 9.4 / 16.6 ms |
| FM: a note, 66 ms read through a Blackman-Harris window at 48 places | 2.4, 2.7 ms | 3.6 ms | 4.4, 4.8 ms | 6.2 ms |
| Transient shaper on today's hit, 0.4 s (renderer and render) | 2.6, 2.9 ms | 3.8 ms | 4.6, 5.4 ms | 7.1 ms |
| Drum hit of 1 s (today's `p.hit`) | 2.7, 3.2 ms | 4.1 ms | 5.2, 6.1 ms | 8.0 ms |
| Polysynth note of 0.3 s (today's `p.hit`) | 1.8, 2.0 ms | 2.7 ms | 3.1, 3.2 ms | 4.1 ms |

**The multiband's quiet click** (`proto_multiband_quiet`): a click at −80 dB through a multiband whose three thresholds are at their lowest, −60 dB, with ratio 20, draws the same curve as one whose thresholds are at 0 dB, to 0.0000 dB at all 256 points, and passes CARD-82's checks at rest. So `p.response({ level: -80 })` draws the split "as it sounds while quieter than its thresholds" (CARD-115) at every setting.

**The first draft's numbers, kept for what they showed** (medians, `proto_tests.rs`; the skeptic reran them within about 5%, and up to 18% higher under load):
- A Wavetable note costs 8.5 ms (10.1 under load) at any length to 0.3 s: its kernel makes its table (32 frames, 9 mip levels, by inverse FFTs) at its first voice's birth in each sandbox. Nine separate notes cost 77.8 ms. This is why the Wavetable stays a sum.
- A compressor at an attack of 1 s needs 5 s at each level (161.6 ms for 13 levels); read after 0.1 s it draws −1.3 dB out at 0 dB in, against −13.4. With attack and release set fast it gives, in 4.2 ms, exactly its kernel's target curve, which the 25-line sum already draws (M2-8). This is why the compressor stays a sum.
- A spectrum at 256 points costs 0.17 ms for 2,048 samples, 1.35 ms for 32,768, and 60 ms for 576,000 (12 s): why a spectrum reads 2 s at most.

**What a 30-card song costs at open** (M2-5), at 48 kHz, base M1 p99 (est.), if the narrowed plan were built. Only pictures on screen measure, as they do today (CARD-69); words measure nothing (D4). Worst case, every card's face shown at once:

| Cards | Each (est.) | In all |
|---|---|---|
| 8 EQs (measured today too) | 8.4 ms | 67 ms |
| 4 Drums (today too) | 4.1 ms | 16 ms |
| 2 polysynth notes (today too) | 2.7 ms | 5 ms |
| 2 reverbs, one at size 1 | 7.7 and 43 ms | 51 ms |
| 1 multiband | 10.1 ms | 10 ms |
| 1 transient shaper | 3.8 ms | 4 ms |
| 1 FM | 3.6 ms | 4 ms |
| 2 Wavetables, 6 compressors, a delay, Pump, 2 others | sums: about 1 ms each | 12 ms |
| **30 cards** | | **about 170 ms** on the pictures thread, once, of which about 90 ms is measured today already; about 340 ms at 96 kHz |

## 6. Is it worth it?

### 6.1 The narrowed `measure`, counted

| | Core (SDK, runner, copy, server) | Packs and std |
|---|---|---|
| **Names** | +8 (the field, `click`, `step`, `note`, `columns`, `spectrum`, `seconds`, `from`); −2 (`p.response`, `p.hit`) | |
| **Lines** | about +550: SDK types and checks of asks +120, `p.response` and `p.hit` −60, the asks checked again in Rust +100, the runner's second call and frozen `heard` +60, the copy (a stepped control lane, columns, a windowed spectrum, `from`, refusals) +230, the server (asks in place of shapes, a 60 ms budget, 30 a second, a shared measure in progress, a bigger cache) +100. The skeptic guessed 800 to 1,200 for the first draft's 14 names (M2-15); this is about half the names and none of `set` | about −545 lines of sums (reverb 240, transient shaper 180, FM 80, multiband 45); about +100 back (std's `curve`, `note`, a backward sum, reading columns and a spectrum at places; the four pictures' drawing from `heard`): **about −445** |
| **Tests** | about +250 (each input and reading against a known sound, the refusals, the budget, the shared measure) | about −190 (four render tests and the "worked once" test) and +80 (the four pictures drawn from a given `heard`): **about −110** |
| **Contract** | CARD-80 to CARD-86 (about 2,900 words) rewritten and about 3 rules added: about +1,000 words | CARD-113, 115, 118 and 140 lose their sums' descriptions, CARD-125 three checks: about −750 words |
| **UI** | | the big reverb redraws about 20 times a second while you drag its size (today: every frame); the rest the same |

**Weighed the RISC way.** risc.md: "a pack's new minor is cheap; a core change is not". A core line is in every song's build, under the contract, in every pack's way, and slow to take back. I count a core line as about three pack lines. Then the narrowed plan costs about 550 × 3 = 1,650 to save about 445: a clear loss. Counted one for one it is still about +800 core lines (with tests) against about −555 pack lines: the total grows.

**What it would buy, all the same:** one truth for the two most fragile copies (the reverb's 240 lines and the transient shaper's 180), FM right at every rate, two heavy first draws gone, and a tool an outside pack could use to draw a real tail or a real note's partials without copying its kernel.

**What it would cost besides lines:** a big reverb's picture slower while you drag it, up to 43 ms of CPU (est.) each time its size changes, and a greyed picture on a slow laptop now and then.

### 6.2 The recommendation

**Don't build `measure` now. Keep the copies, and take three small steps:**

| Step | Core | Packs and std | Gain |
|---|---|---|---|
| `p.response({ level })`: a click from −80 to −20 dB (today's is −20); CARD-82's checks at a 32nd of it, as now. The multiband's split is measured at −80 dB. | 1 option; about +30 lines and +30 test lines; CARD-80 a sentence longer | −45 lines of sums, −42 of test; CARD-115 and CARD-125 shorter | one copy gone, at no cost to the UI (5.8 ms a drag frame, est.) |
| std's drawn lowpass (CARD-97) goes; no shipped sound uses it | none | −45 lines, −44 of test; CARD-97 gone | less to keep |
| Each copy's render test also runs at 44.1 and 96 kHz | none | tests only | a copy wrong away from 48 kHz is found. If some are, the fix is to give a picture the song's rate (one name, D3) |

**Build the narrowed `measure` (§4) later, when one of these happens** (D5):
- a pack from outside music2 needs to draw a real tail, a real held sound or a real note's partials;
- a copy breaks for real: its kernel changes and its picture lies;
- the copy's table can be kept between measures (`warm`), so the Wavetable can move too.

## 7. Risks

For the narrowed `measure`, if it is built:

| Risk | Answer |
|---|---|
| **A sound that isn't the same each time** (noise, a random detune) | Kernels are deterministic (RENDER-33) and a copy plays from sample 0, so a measure is the same each time; CARD-84's keeping already relies on it. |
| **A very long tail** | Cut at 12 s. The backward sum then reads a little low near its end: under 2 dB at the −60 dB point of the biggest room (estimate), nothing at its −30 dB dot. The words don't claim the cut part. |
| **The dry sound in a reverb's click** | Read from 1 ms on (`from: 0.001`), so the −30 dB time doesn't move with the mix (M2-2). measure2-fx's test: the tail is the same at mix 0.25 and mix 1. |
| **A value driven by a cable or a lane** | As CARD-81: a click's spectrum is refused ("it has no one curve"); columns play the work as it is copied (an LFO on a value shows its motion: the truth). |
| **A sound that listens to another track** | Its sidechain hears silence (CARD-81). None of the four pictures of §3 listens; the compressor stays a sum. Today's keyed compressor draws its curve against the level it listens to while its dot rides the input's (M2-21): a gap for gaps.md, not this note's. |
| **A slow or busy computer** | CPU-time budgets, as CARD-85; the size-1 reverb greys and is tried again in 10 s past 60 ms. |
| **The audio's cache** | Normal priority, 30 measures a second for each picture, and gate 8's dropout run on the reference Mac before landing (§4.3). |

## 8. Migration

**Now** (if D1 and D2 are yes), one small track, `measure2-small`:
1. The contract: CARD-80 gains `level`; CARD-115 says the split is measured; CARD-97 goes.
2. Test first: `the_multiband_split_is_measured_under_its_thresholds` fails on today's drawing (it has no `response` shape), then passes; a −80 dB click at thresholds −60, ratio 20, draws today's thresholds-0 curve.
3. `p.response({ level })` in `@m2/core` and the copy; the multiband's `split` draws `p.response({ level: -80 })`; its sums and their render test go; `@m2/mixing`'s minor moves.
4. std's lowpass, its words and its test go.
5. The render tests of every copy loop over 44.1, 48 and 96 kHz. Any that fails is recorded in gaps.md with its error in dB, and D3 decides.

Size: small.

**Later** (D5), split as the skeptic asks (M2-16):

| # | Track | Builds | Size | Fails first |
|---|---|---|---|---|
| 1a | `measure2` | The contract; `measure` and its 8 names, typed and checked twice; the runner's second call; the copy's `click` columns, stepped `step`, `note` spectrum; `p.response` and `p.hit` as std TypeScript, with no picture changing | medium | a click read as columns from a reverb at mix 0.25 equals one at mix 1 from 1 ms on |
| 1b | `measure2-drag` | the 60 ms budget, 30 a second, the shared measure in progress, the bigger cache; gate 8's dropout run on the Mac | small | two asks for one picture at open measure once, not twice |
| 2 | `measure2-packs` | the reverb, the transient shaper and the FM partials measured; std's helpers (backward sum, columns, spectrum at places) owned here | medium | each picture drawn at 96 kHz against a render at 96 kHz (today's sums are 48 kHz's) |
| 3 | `warm`, then the Wavetable | keep a copy's sandboxes between measures, so the table is made once; then its frames (unison and smoothing handled, M2-9), filter and envelopes | medium, in the runtime; then medium | a position drag frame under 8 ms (est. base M1) |

## 9. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | Some cards draw their picture from a copy of the sound's maths, checked against the real sound by a test. We could add a measuring tool to music2's core so four of them measure the real sound instead. It makes the core bigger than the copies it removes, and a huge reverb's picture would follow your drag less smoothly. Keep the copies for now? | **Yes, keep them.** The tool costs more than it saves today. |
| D2 | The multiband compressor's picture could be measured from the real sound by playing a very quiet click through it, with one small option added to what already draws the EQ's curve. Do it? | **Yes.** One copy gone, and the picture still follows your drag every frame. |
| D3 | If a picture turns out wrong at 96 kHz, should pictures be told the song's sample rate so they can be right? (One small addition.) | **Yes, if the tests find one.** |
| D4 | If the measuring tool is built later: a reverb card's one-line words would say how big the room is ("a big room, size 0.9"), not how long it rings; the picture still shows how long. All right? | **Yes.** Otherwise every card's words must be measured after every change. |
| D5 | Build the measuring tool later, only when a pack from outside needs it, a copy breaks, or the Wavetable can be measured fast? | **Yes.** |
| D6 | The Wavetable's pictures stay drawn from their copy until measuring it is fast. Making it fast is its own piece of work. Do that now or later? | **Later.** Today's drawing is fast and right. |
| D7 | Chorus and phaser keep their small moving drawing. All right? | **Yes.** |
| D8 | Old std's drawing of a filter, which no sound uses any more, is deleted. All right? | **Yes.** |

## 10. What the core gains and loses, counted

| Plan | Core names | Core lines (code, tests) | Packs and std lines (code, tests) | UI |
|---|---|---|---|---|
| **Recommended: keep the copies, three small steps** | +1 option (`level`) | +30, +30 | −90, −86 | the same |
| The narrowed `measure` (§4) | +8 names, −2 words | about +550, +250 | about −445, −110 | the big reverb slower while dragged |
| The first draft (rejected by the review) | +14 names, −2 words | about +540 counted then; the skeptic estimates 800 to 1,200 | about −750, −400 | the Wavetable worse while dragged |

## 11. Housekeeping

Prototype sources, kept outside the repo: `$SCRATCH/measure2-note/proto_tests.rs`, `proto2_tests.rs` and `measure-raw.patch` (and the skeptic's tree under `review/`). `proto_multiband_quiet` also needs `measure.rs`'s `LEVEL` set to 0.0001 (−80 dB), which the patch doesn't do. Their target folders were deleted.

## 12. The review, answered

| Id | Finding | Answer |
|---|---|---|
| M2-1 | `step` with `levels` doesn't say click or steady; the reverb and delay would draw flat | Changed: `click` and `step` are two inputs (§4.2); "until quiet" is the click's alone; the examples and the reverb's costs are fixed (§4.1, §5: 4.7 ms default room, listening 1.86 s) |
| M2-2 | The reverb's tail includes the dry sound | Changed: the tail reads from 1 ms on (`from: 0.001`), with a test at two mixes (§7) |
| M2-3 | The Wavetable gets worse while you drag | Changed: the Wavetable stays a sum; `warm` comes first, in its own track (§3, §8, D6) |
| M2-4 | 14 names for little | Changed: 8 names (§4.2), counted against what each deletes (§6.1); `tone`, `left`, `of`, `each` as a word, `held` and `set` dropped; widen and the rest keep their sums |
| M2-5 | The budget's margin; a fast CPU; the cost at open | Changed: p99 from 100 runs, scaled ×1.3 to a base M1 with the reason (§5); 60 ms budget, 30% margin for the biggest room; what a slow laptop gets; a 30-card song at open, about 170 ms (§5) |
| M2-6 | D6's words measure everything on one thread, keyed wrong | Changed: words read no measure (D4); a measure in progress is shared; the cache is 256, at rest apart from drags (§4.3) |
| M2-7 | The fitted line is honest only for a reverb; partial answers | Changed: no partial answers, no fitted line; a slow measure redraws up to 30 times a second (§4.3) |
| M2-8 | `set` measures another sound; long lists copy the kernel | Changed: `set` dropped; `only: [names]` is the form if a picture ever needs it; the compressor stays a sum, its words "the curve it aims for" (§3, §4.2) |
| M2-9 | Wavetable frames: unison, smoothing, halfband, rate | Agreed; the Wavetable stays a sum until `warm`, and that track must handle each (§8, track 3) |
| M2-10 | Two asks depend on the rate | Changed: the bitcrusher stays a sum and no ask in §3 depends on the rate (§4.2) |
| M2-11 | A spectrum of a note has no window | Changed: Blackman-Harris, at least 4 ÷ the closest spacing, refused shorter; FM re-timed with it, 2.7 ms p99 here (§4.2, §5) |
| M2-12 | The multiband at −60 dB sits on its threshold | Changed: a click at −80 dB; prototyped at thresholds −60, ratio 20: the same curve as thresholds 0, to 0.0000 dB, and CARD-82's checks pass (§5) |
| M2-13 | D8 isn't needed: a list can play today's hit | Changed: the transient shaper keeps today's hit, played as a list of levels; D8 dropped (§4.1) |
| M2-14 | The delay's echoes past the cap, and 26 dB low by mean power | Agreed; the delay stays a sum. `columns` gives each column's loudest sample beside its power, should it ever be measured (§4.2) |
| M2-15 | The core's gain counted too small; D11 counted nowhere | Changed: recounted by part, about +550 core and +250 core test lines for 8 names; `m2 pack check` measuring (old D11) costed at about 100 lines and left out (§4.3, §6.1) |
| M2-16 | Track 1 too big; `warm` too late; std's helpers unowned; "fails first" vague | Changed: tracks 1a, 1b, 2, then `warm` before the Wavetable; std's helpers owned by track 2; a concrete first failure for each (§8) |
| M2-17 | Decisions a beginner can't answer | Changed: every decision asks about what you see or hear (§9) |
| M2-18 | Thread priority and the audio's cache | Changed: normal priority and why not background; 30 measures a second; gate 8's dropout run on the Mac before track 1b lands (§4.3) |
| M2-19 | Envelopes through a note; the 12 s cap | Changed: envelopes stay drawn from their values (§3) |
| M2-20 | Copies left behind, uncounted | Changed: a list of the copies that stay, each with its test (§3) |
| M2-21 | A keyed compressor's dot rides the wrong level | Agreed, and it is true today; the compressor stays a sum, so it goes to gaps.md as a gap of its own (§7) |
