# Cards you play with: a face, tabs, and pictures that packs draw

| | |
|---|---|
| **Status** | A proposal, for the owner's yes (AGENTS.md rule 11). Nothing here is built. |
| **Written** | 2026-11-06, from `main` at `de95d95a` |
| **Builds on** | [Typed sounds](../cards/design.md) (accepted 2026-10-31): values typed, "the type says what, never how to draw it" (§2.2), views (§3.2), groups (§3.3), taps (§5), and §9's "no pack code in the window" |
| **Gaps** | #59 (a spectrum view and the EQ's curve); the filter view that draws only a low-pass (gaps.md, Not built yet) |
| **Mockup** | `$SCRATCH/cards2-note/mockup.html`: five cards you can drag, made with fake sound ([picture](images/mockup.png)) |

**In short**
- **Today a card is a long list.** One knob per row, top to bottom. The Parametric EQ is 40 rows and 1,872 px tall, with no picture of its curve. Nothing you drag shows what you hear.
- **The new card has a face and tabs.** The face holds the one or two pictures that matter, drawn big, and the four to six knobs a beginner needs. You drag the picture itself: the EQ's dots over its live spectrum, the compressor's curve, the envelope's corners, the wavetable, the drum's hit, Pump's dip. Everything else goes in tabs, and a card's groups become its tabs. The last tab, All, lists every value.
- **A card has three sizes.** Folded is one line and a small picture. Normal is the face and one tab. Expanded is the face drawn bigger, with every tab side by side.
- **Recommended standard for packs (option B):** music2 still draws every card. A pack may add **pictures**: TypeScript that turns the card's values into a list of shapes (lines, areas, dots, labels). The pack's code runs where it runs today, in the build's sandbox on the server, never in the window. music2 draws the shapes in its own style. A dot names the values it moves, so music2 does every drag, write and undo step. Live layers (a spectrum, a level, gain reduction) come from taps that music2 draws itself.
- **This keeps §9 and changes §2.2.** No pack code runs in the window, so §9 stands. But a pack can now draw a picture of its own, made from a small, fixed set of shapes. Knobs, menus and switches stay music2's alone.
- **What the core gains:** one view kind (`view.picture`), about eight shape words, one descriptor field (`editor.face`), and two kinds of tap (a spectrum and a level). No new kernels and no new routes. A pack draws every new picture without a core change.

## 1. What is wrong today

These pictures show the app at `de95d95a`: `examples/first-song`, with a Parametric EQ, Compress, Reverb and Pump added after the keys, and a Wavetable track. They were taken in headless Chrome from the headless server, in a scratch `HOME`.

![The Keys chain in the dock at 75%](images/today-window.png)

*The dock at its usual size. Titles are cut to "P.", "Com…" and "R", and the knobs are 21 px at the 75% zoom.*

![The same chain at 100%, the dock made tall](images/today-keys-chain.png)

*The same chain at 100%. The EQ is one column of 40 rows, 1,872 px tall.*

![The Wavetable's card](images/today-wavetable.png)

*The Wavetable is 35 rows, 1,768 px tall. Its two envelopes and its filter are three small pictures among the rows.*

![The Drum's card](images/today-drum.png)

*The Drum: a menu and four knobs. It has no picture of the hit.*

| What is wrong | Where you see it |
|---|---|
| **A list, not an instrument.** Every value is a row: a label, a number, a 28 px knob. Nothing is bigger than anything else, so nothing tells you where to start. | every card |
| **No picture where one matters.** The EQ has no curve and no spectrum (gap #59). The compressor has no curve and no gain-reduction meter. Pump doesn't show its dip, the Wavetable doesn't show its table, and the Drum doesn't show its hit. | EQ, Compress, Pump, Wavetable, Drum |
| **The pictures we have are small and fixed.** There are four: envelope, filter, keys and scope. The filter view draws a 12 dB per octave low-pass, whatever the filter is. A new picture means a change to the core's closed list (CARD-16). | Wavetable's filter |
| **Groups are headings in a column.** The EQ's eight bands are eight headings, one under the other. Folding hides every group but the first. | EQ, Multiband, Wavetable |
| **The patch shows through.** Every row has a port diamond and the card has an "In audio" row: cables you rarely want in a device chain. | every card in Device |
| **It doesn't fit.** At the dock's 300 px a card shows six rows. Fit zooms out to 75%, and titles are cut to one letter. | the dock |
| **Nothing moves with the music**, except a scope you must open. | every card |

What is good, and stays:
- One declaration (the descriptor) still drives the card, `m2 graph` and the API.
- A change still writes the code, with one undo step.
- Knobs keep the gestures in taste.md §5.2: ⇧ for a fine drag, double-click to type, ⌥-click to reset.

## 2. What the best devices do (research)

Sources are the pages read on 2026-11-06.

| Device | What it does that people love | Source |
|---|---|---|
| Ableton EQ Eight | Drag a dot: across for frequency, up and down for gain. ⌥-drag for Q, and up and down for Q on a cut. A spectrum under the curve. The expanded view puts all eight bands in the main window. | [Live 12 audio effects](https://www.ableton.com/en/live-manual/12/live-audio-effect-reference/) |
| Ableton Compressor | Three views in one display: Collapsed (only the main knobs), Transfer Curve, and Activity (input grey, reduction orange). A gain-reduction meter. | same |
| Ableton Wavetable | You drag the table display to move through it. The filter has dots to drag. Tabs: Osc, Matrix, MIDI, Mod Sources. | [Live 12 instruments](https://www.ableton.com/en/live-manual/12/live-instrument-reference/) |
| Ableton, every device | One title bar: on/off, fold with a double-click, swap. Signal flows left to right, and an arrow toggles each device's expanded view. | [Working with instruments and effects](https://www.ableton.com/en/live-manual/12/working-with-instruments-and-effects/) |
| Ableton Racks | Up to 16 macros, 8 shown. Map one to any knob with a range. Variations save a set of macro settings. | [Racks](https://www.ableton.com/en/live-manual/12/instrument-drum-and-effect-racks/) |
| Bitwig | One gesture modulates any knob: click a modulator's port, then drag the target knob. Live markers show the modulated value. | [Unified modulation](https://www.bitwig.com/userguide/latest/the_unified_modulation_system/) |
| FabFilter Pro-Q 4 | The curve is the interface. Click to make a band (its shape depends on where you click), drag it, scroll for Q. A band panel floats under the band you picked. Spectrum Grab and a piano scale. | [Pro-Q EQ display](https://www.fabfilter.com/help/pro-q/using/eqdisplay), [band controls](https://www.fabfilter.com/help/pro-q/using/bandcontrols) |
| FabFilter Pro-C | A knee display (in against out) that lights with the live level. A big level history: in, out, and reduction as a line. A compact mode hides the displays. | [Pro-C displays](https://www.fabfilter.com/help/pro-c/using/displays) |
| Vital, Serum | Animated waveforms, filters and modulation at 60 fps. Drag a source onto a knob, and a ring on the knob shows the motion. | [vital.audio](https://vital.audio/), [Serum 2 knobs](https://xferrecords.com/web-manual/serum-2/using-knobs-and-sliders) |
| Logic Channel EQ | Drag the shaded band. An analyser over the curve, before or after the EQ. Each band's colour matches its button. | [Channel EQ](https://help.apple.com/logicpro/mac/10.4.5/en.lproj/lgcef1edc1d7.html) |

**Why they are loved:**
- **Direct manipulation.** You act on the thing you see, in small steps you can undo ([NN/g](https://www.nngroup.com/articles/direct-manipulation/)).
- **Live feedback.** The picture moves with the sound.
- **Progressive disclosure.** A simple face, with more one level down. NN/g advises two levels at most ([NN/g](https://www.nngroup.com/articles/progressive-disclosure/)).
- **Consistency.** One title bar and one modifier set everywhere.
- Sound On Sound says Pro-Q "reimagined the plug-in equaliser using the principles of good software interface design" ([review](https://soundonsound.com/reviews/fabfilter-pro-q-4)).

**How hosts let other people draw:**

| Host | How | The trade |
|---|---|---|
| VST3, CLAP | The plugin opens its own window and draws it with any code it likes; the host only sizes it ([CLAP gui.h](https://raw.githubusercontent.com/free-audio/clap/main/include/clap/ext/gui.h)). | Total freedom. No consistency, native code fully trusted, and each plugin must make itself accessible ([CLAP #225](https://github.com/free-audio/clap/discussions/225)). |
| Bitwig devices | The host draws every native device. Outsiders get remote-control pages, Grid patches and controller scripts ([Bitwig](https://bitwig.com/userguide/latest/midi_controllers)). | Perfect consistency, but no outsider can make a new face. |
| Max for Live | `live.*` widgets become Live parameters in Live's own colours. `v8ui` draws freely in JavaScript ([v8ui](https://docs.cycling74.com/reference/v8ui), [parameters](https://docs.cycling74.com/userguide/m4l/live_parameters)). | A kit gives consistency. Custom drawing loses the theme unless the author works at it. |
| REAPER JSFX | `slider` lines make an automatic UI. `@gfx` is a small drawing API the host runs about 30 times a second, on a thread apart from the audio ([JSFX](https://www.reaper.fm/sdk/js/js.php), [gfx](https://www.reaper.fm/sdk/js/gfx.php)). | Cheap and safe, but it draws pixels, so every author's looks different. |
| iPlug2, NIH-plug, Elementary | The plugin's own webview, talking to the host in JSON ([iPlug2](https://iplug2.github.io/docs/class_web_view_editor_delegate.html), [nih-plug-webview](https://github.com/maxjvh/nih-plug-webview), [SRVB](https://github.com/elemaudio/srvb)). | Quick to build with web tools. It is still the plugin's own window, heavy, and unchecked. |
| The web | A sandboxed iframe with `postMessage`, an OffscreenCanvas in a worker, or Shadow DOM ([iframe](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/iframe), [OffscreenCanvas](https://developer.mozilla.org/en-US/docs/Web/API/OffscreenCanvas), [Shadow DOM](https://developer.mozilla.org/en-US/docs/Web/API/Web_components/Using_shadow_DOM)) | The iframe is a real wall. A worker draws pixels no one else can read. Shadow DOM is no wall at all. |
| Faust | An abstract widget tree (`hslider`, `tgroup` for tabs, metadata such as `[unit:dB]`, `[scale:log]`) that any host draws ([Faust](https://faustdoc.grame.fr/manual/syntax/)). | Any host can draw it, automate it and make it accessible, but it can't draw a picture of its own. |
| Figma, VS Code | Declared UI first. A sandboxed webview only "when absolutely necessary" ([VS Code](https://code.visualstudio.com/api/ux-guidelines/webviews), [Figma](https://developers.figma.com/docs/plugins/how-plugins-run/)). | Consistent and accessible. The escape hatch is discouraged. |

**The pattern:** the systems people like best start from a typed description the host draws. Some add a narrow, sandboxed way to draw, and every value it changes is still a host parameter.

## 3. The new card

### 3.1 Its parts

```
┌──────────────────────────────────────────────┐
│ Parametric EQ  @m2/mixing        [–][▢][⤢]   │  header: name, its pack, three sizes
├──────────────────────────────────────────────┤
│  ┌────────────────────────────────────────┐  │
│  │  spectrum (live) under the curve       │  │  the face: the picture that matters,
│  │      ②──③      ⑤──⑥                    │  │  big; you drag the dots
│  │  ①/        ╲④╱        ⑦──⑧             │  │
│  └────────────────────────────────────────┘  │
│  [•1 •2 •3 •4 •5 •6 •7 ∘8 | All]             │  tabs: the descriptor's groups, then All
│  On ●  Shape [bell ▾]   (Freq) (Gain) (Q)    │  the tab's values
├──────────────────────────────────────────────┤
│ eq(pluck(…), { gain4: -3.5, freq4: 800 })    │  the code it writes (Expanded and Normal)
└──────────────────────────────────────────────┘
```

| Part | What it is | Declared by |
|---|---|---|
| **Header** | The name, the pack it comes from, ⋯ (as today) and the three sizes | the descriptor's `label` |
| **Face** | At most two pictures and at most six values, drawn big | `editor.face: ["curve", "cutoff", …]` (**new**); with no face, the first group is the face |
| **Tabs** | One tab for each group, in their order, then **All** | `editor.groups` (as today, CARD-17) |
| **All** | Every value in one list, with its home and its id for `m2 set`: what `m2 graph` prints and what an agent reads | nothing: every card has it |
| **The code line** | The call as the code writes it, updated as you drag | nothing |

- A picture's dot can name the tab it **opens**. Picking EQ band 3's dot shows the "3" tab, and picking the tab marks the dot.
- A tab whose values include a switch named `on…` shows that switch as a dot on the tab, filled when it is on (the EQ's bands). This is presentation; it changes nothing.
- The ports and the "In audio" row leave the Device chain. They stay in Patch, where cables are the point (taste.md §3.5 already says Device is a chain).

### 3.2 Knobs

- They keep taste.md §2.8 and §5.2: a 270° arc, 300 px for the full range, ⇧ for fine, double-click to type, ⌥-click to reset, and the value bubble while you drag.
- **In a grid, not a list.** Columns of 64 px hold the knob, its label and its value. A face's knobs are 36 px (`big`); a tab's are 28 px.
- **Menus with five options or fewer** are a segmented control (Pump's rate, the filter's shape). Longer ones stay menus. A choice of many related options, such as the Drum's 17 drums, is a grid of chips grouped by family.
- **A switch** is a switch, as today.
- **A recording** is its waveform, as today: drop a file on it.
- **Defaults** show in `fg-muted`, and a written value in `fg` (HOME-1, taste.md §2.10).

### 3.3 Sizes

| Size | Shows | Width (normal; a phone is always full width) |
|---|---|---|
| **Folded** | The header, one sentence ("7 bands on · −3.5 dB at 800 Hz") and its first picture, 40 px tall and still | 240 px |
| **Normal** | The face, the tab bar and one tab | the card's own (EQ 460, Compress 480, Wavetable 440, Drum and Pump 380) |
| **Expanded** | The face drawn bigger, every tab side by side, and All behind a disclosure | about twice the normal width |

- The size is the window's own, like today's fold (`web/src/lib/folded.ts`). It is not the song's, so it writes nothing.
- The sentence comes from the picture's `words` (§4.4).

### 3.4 Keyboard

| Where | Key | Does |
|---|---|---|
| a knob | ↑ ↓ (→ ←) | 1% of its range; with ⇧ 0.1% |
| | PageUp, PageDown | 10% |
| | Home, End, ⌫ | its lowest, its highest, its default |
| | Enter (double-click) | type a value: "−3", "2k", "120ms" |
| a picture's dot | ← → | its x value (the EQ: a sixth of an octave) |
| | ↑ ↓ | its y value (the EQ: 0.5 dB) |
| | ⌥↑ ⌥↓ | its wheel value (the EQ: Q) |
| | Enter, Space | its `toggles` value (the EQ: the band on or off) |
| the tab bar | ← → | the next or the previous tab |
| a card | Tab | header, face dots, face knobs, tabs, the tab's values, in that order |

The size buttons are in the tab order. A shortcut for them waits for taste.md §5.1's map (D7).

### 3.5 What an agent sees

The API must still show everything (AGENT rules; CARD-1, CARD-25, CARD-26).
- **Every value** is in the view as today, whatever tab it is on. All is that same list, drawn.
- **The face and the tabs** are in the view (`ViewNode.face`, `ViewNode.groups`), and `m2 graph` prints them as headings.
- **Every picture is data.** The view carries its shapes and its words: the curve's points, each dot's place and the values it moves, and "Band 4: bell, −3.5 dB at 800 Hz". An agent reads the EQ's curve as numbers, never from pixels. `m2 graph` prints the words.
- **Live layers** are taps an agent can ask for as a window does (TAP-8): an agent's spectrum ask gets the bins in each answer.
- **A value on a hidden tab** that an agent changes lights its tab. An agent's pointer at it shows on the tab, as a folded card's "N more" row does today (CARD-25, review V8).

## 4. The standard for packs

### 4.1 The question

The owner asked for "a standard so that 3rd party packages can design the cards from scratch". Three answers:

| | What a pack can do | Where its code runs |
|---|---|---|
| **A. More host views** | Pick from a bigger list of pictures music2 draws: an EQ curve, a transfer curve, a spectrum, an XY pad, a meter, a wavetable, a step grid, tabs | nowhere: it only names them |
| **B. Pictures of its own** | Option A's card, plus pictures it draws: TypeScript that turns the card's values into shapes, which music2 draws. A dot names the values it moves; music2 does the drag, the write and the undo. | **B2 (recommended):** in the build's sandbox on the server (QuickJS, as the build runs pack code today). **B1:** in the window, in a sandboxed worker with a canvas. |
| **C. Its own HTML and JavaScript** | Anything: the whole card is the pack's web page | in the window, in a sandboxed iframe |

### 4.2 How they compare

| | A. More host views | B2. Shapes, drawn by music2 | B1. Pixels in a worker | C. Own HTML |
|---|---|---|---|---|
| **For a beginner** | Every card looks and works the same | The same: music2 draws every shape, knob and dot in its style, with its gestures | Pictures look like whatever each author drew | Every pack is a new app to learn |
| **RISC: what the core grows by** | One core widget for each new kind of picture (about eight now, more for each new idea); each one is Rust and React, a rule and tests | One view kind and about eight shape words, **once**; every picture after that is a pack's TypeScript | One view kind, a worker host and a canvas API | A web host, a message protocol and a permission model |
| **Safety: a stranger's code** | none runs | none in the window. It runs where pack code already runs: QuickJS with a memory and time limit, no files, no network, no clock, no randomness. It can't write: it returns shapes. | A stranger's code in the window, walled off by the worker, but it can spin the CPU and must be killed by the host | A stranger's page in the window. A CSP and the sandbox attribute wall it off, but it is a full browser surface |
| **Agents see what the musician sees** | Yes: the view's data | **Yes: the shapes are data**, with words | No: pixels only | No |
| **The work** | Medium for each picture, without end | Large once (about the size of track `views`), then small for each picture, done in its pack | Large once; each picture is small, but drawing code is long | Largest; it also needs a review process for packs |
| **How close to Ableton and FabFilter** | Only as close as the host's list allows; a new idea waits for a core release | Close: a pack can draw its own maths (the EQ's true curve, its own filter slopes) over live taps | Closest in pixels | As close as the author's skill |

### 4.3 Recommendation: B2

- **music2 draws the card; a pack may draw pictures, as shapes.**
- It is the RISC answer. The core gives one small, general primitive, and every picture is TypeScript in a pack: the EQ's curve in `@m2/mixing`, the wavetable in `@m2/wavetable`, the common ones (envelope, filter, transfer curve) in std.
- It keeps every first principle of the accepted design. One declaration (§2.1). Every value has one home (§2.3): a picture holds no value, and its dots move values that have homes. Looking costs nothing (§2.7): live layers are taps.
- It changes one thing: §2.2's "a pack can't add a widget" becomes "a pack can't add a widget **or a shape**". Knobs, menus, switches and the shape words stay closed; a picture made from them is open.

**How it changes the accepted design (the owner should see this plainly):**

| The accepted design said | This proposal |
|---|---|
| §9: "Packs bring their own window code (HTML and JavaScript)": rejected, because of strangers' code in the window, packs that all look different, and agents who can't see in | **Kept.** No pack code runs in the window. A picture's code runs in the server's sandbox, and the window draws only shapes in music2's own style. Agents read the shapes. Option C stays rejected for the same reasons. |
| §2.2: "A pack can't add a widget: the vocabulary is closed and small" | **Changed in part.** The widgets (knob, menu, switch, number box, recording) and the shape words are still closed. A pack can now combine shapes into a picture of its own. |
| §3.2: four views, `envelope`, `filter`, `keys`, `scope` | **Kept**, plus `view.picture`. `envelope` and `filter` become std pictures written over it (D5), so the window has one renderer. `keys` (it plays audition) and `scope` stay music2's. |
| CARD-16, CARD-27: the filter view draws a 12 dB per octave low-pass, and says it is a drawing | **Better.** A pack's picture draws its own filter's real curve, since it knows its own maths. |

### 4.4 What a picture is

```ts
import { view, axis, line, area, handle, mark, label, tap } from "@m2/core";

curve: view.picture({
  x: axis.log("hz", 20, 20000),          // music2 draws the grid and its labels
  y: axis.lin("db", -24, 24),
  live: [tap.spectrum("in"), tap.spectrum("out")],   // drawn by music2, at the screen's rate
  draw: (v) => [
    line(curvePoints(v)),                // the pack's own maths
    ...BANDS.map((b) => handle(v[`freq${b}`], v[`gain${b}`], {
      x: `freq${b}`, y: `gain${b}`, wheel: `q${b}`, toggles: `on${b}`,
      off: !v[`on${b}`], opens: `Band ${b}`, mark: `${b}`,
    })),
  ],
  words: (v) => saidBands(v),            // "Band 4: bell, −3.5 dB at 800 Hz; …"
}),
```

| Word | Says | Notes |
|---|---|---|
| `axis.lin(unit, lo, hi)`, `axis.log(…)` | the picture's x and y, in a unit | music2 draws the grid and labels as taste.md formats them |
| `line(points, tone?)` | a line through points, in the axes' units | `tone` is `main`, `soft` or `faint`: never a colour |
| `area(points, tone?)` | the same, filled down to the axis | |
| `handle(x, y, moves)` | a dot you can drag | `moves`: `x`, `y`, `wheel`, `toggles` (values), `opens` (a tab), `mark` (a short label), `off` |
| `mark(axis, at, words?)` | a thin guide line (a threshold, a crossover) | |
| `label(x, y, words)` | a few words | |
| `live: tap.spectrum(point)`, `tap.level(point)`, `tap.scope(point)` | a layer music2 draws from a tap | `point`: `in` or `out` of the call |
| `words(values)` | one sentence for the folded card, `m2 graph`, agents and screen readers | required |

The rules:

1. **A dot moves only this card's values, checked by type.** `x: "freq3"` must name a control of this descriptor whose unit fits the axis (Hz on a Hz axis), as CARD-16 checks a view's names. TypeScript checks it as you type, and the build checks it again. A dot may instead move a value **as its knob does** (`y: knob("ratio")`), for a value that isn't on an axis.
2. **music2 does the gesture.** A drag is a knob's gesture: leases, preview, one `Values` Tx at release, one undo step (CARD-22). It moves only in the direction the pointer went and stays in the knob's range (CARD-27). A pack never writes anything.
3. **`draw` is a pure function of the values** (and the song's tempo and sample rate, and a recording's frames when the picture names one). It runs in a QuickJS sandbox for the pack on the server, as the build runs pack code today (`core/script`: a memory limit and an interrupt). It runs when a value changes, and on each preview while a dot is dragged. It never runs per audio block.
4. **Over its time (10 ms) or its memory, a picture stops.** It greys and says why in words ("Parametric EQ's picture took too long and was stopped"), and the card's knobs still work, as a kernel past its deadline does (ABI-6).
5. **Live layers never call pack code.** The server sends a tap's numbers about 30 times a second while the picture shows (TAP-4, TAP-8), and the window draws them under the pack's shapes.
6. **music2 draws in its own style.** Hairlines, its greys, the accent only on the picked dot, no gradients (taste.md §2.8). A pack chooses a tone, never a colour.

While you drag, the window moves the dot at once, since it knows the axis. The curve follows when the server's new shapes arrive, a few milliseconds later on the same computer.

## 5. Card by card

This covers every component shipped today, from `std/src/*.ts` and `packs/@m2/*/src`. **Face** is what the face draws. **Tabs** are its groups. **Pictures** says where each picture comes from: *std* means `std/pictures` (TypeScript, new), *pack* means the pack's own code, *host* means music2's.

### 5.1 Instruments

| Component | Face | Tabs | Pictures |
|---|---|---|---|
| `@m2/wavetable` Wavetable | the table A display (drag across for position; the LFO's reach drawn on it); Position, Cutoff, Attack, Release | Osc A, Osc B, Filter, Env (amp and filter), LFO, More, All | table (pack: the frames from its tables, or a dropped file's frames), filter (pack: its real lowpass, bandpass, highpass and notch), envelope ×2 (std), LFO shape (std) |
| `@m2/synths` Polysynth (`synth-custom`) | the stack's wave (shape, saw, pulse, sub, noise mixed) and the amp envelope; Shape, Cutoff, Resonance, Attack, Release | Oscillators, Filter, Loudness, Pitch, All | wave (pack), filter (pack: its 24 dB ladder), envelope (std) |
| Supersaw, Lead, Synth pad, Strings | filter curve and amp envelope; Shape and two of its Sound knobs; Cutoff | Sound, Filter, Loudness, All | filter (pack), envelope (std) |
| Synth pluck, 808 sub, Synth bass | amp envelope; its Sound knobs | Sound, (Filter), Loudness, All | envelope (std), filter (pack, bass) |
| `@m2/synths` FM synth (`fm-custom`) | pair A and B as two small spectra of their partials, and the amp envelope | Pair A, Pair B, Tone, Loudness, All | partials (pack), envelope (std) |
| FM keys, FM bell | amp envelope; Tone, Tine | Sound, Loudness, All | envelope (std) |
| FM metal | its decay as a falling line; Tone, Feedback, Drop | Sound, Loudness, All | decay (pack) |
| `@m2/drums` Drum | the hit: its envelope from the pack's maths, and the last hit's waveform (a live layer); drag its end for decay and its start for punch; click to hear it. The drums as chips by family; Tune, Decay, Punch, Drive | All | hit (pack), last hit (host: a scope, D9) |
| `@m2/drums` Drum machine (unlisted) | the kit's drums as rows, each with its level | Kick, Snare & clap, Hats, Toms, Percussion, Whole kit, All | none |
| std Sampler | the recording's waveform with its root note, or the key map (CARD-28) | Sound, Keys, All | waveform (host, as today), keys (host) |
| `@m2/synths` Bass, Pluck, Pad (unlisted, simple) | knobs only | All | none |

### 5.2 Effects

| Component | Face | Tabs | Pictures |
|---|---|---|---|
| `@m2/mixing` Parametric EQ | the curve over the live spectrum (in and out); eight dots: drag, scroll for Q, double-click to turn on or off | 1 to 8 (a band's On, Shape, Frequency, Gain, Q), All | curve (pack), spectrum (host tap) |
| std EQ3 | the three-band curve over the spectrum; three dots (low, mid, high: frequency and gain) | All | curve (std), spectrum (host tap) |
| std Compress | the transfer curve (drag the corner for threshold, the line for ratio) with the live level dot and a gain-reduction meter; beside it the level history; the five knobs | Listens to, All | transfer (std), level and reduction (host taps) |
| `@m2/mixing` Multiband compressor | the spectrum split in three at the two crossovers (drag the marks); each band's gain reduction as a bar | Low, Mid, High, All | split (pack), spectrum and levels (host taps) |
| `@m2/mixing` Gate | the level history with the threshold line (drag it) and the gate's open and shut | Listens to, All | history (std), level (host tap) |
| `@m2/mixing` Transient shaper | one hit drawn before and after (attack and sustain, dB) | All | shape (pack) |
| `@m2/mixing` De-esser | the spectrum with the band around Frequency (drag it) and its reduction | All | band (pack), spectrum (host tap) |
| `@m2/mixing` Pump | one bar of the dip (drag its foot for depth, its curve for shape) with the playhead, and the rate as a segmented control | All | dip (pack: its own formula), playhead (host) |
| std Duck | the level history of what it listens to, with the dip | Listens to, All | history (std), level (host tap) |
| std Limit | the level history with the ceiling line (drag it) | All | history (std) |
| `@m2/effects` Delay | the echoes as falling bars in time (drag the first for time, the slope for feedback); Mix | All | echoes (pack) |
| `@m2/effects` Reverb | the tail's shape (size its length, damping its colour); Mix | Room, Mix, All | tail (pack) |
| `@m2/effects` Chorus, Phaser, Tremolo | its motion over a second (rate, depth; the phaser's notches) | Sweep, Mix (phaser), All | motion (pack) |
| `@m2/effects` Drive | the shaping curve, in against out | All | curve (pack) |
| `@m2/effects` Bitcrush | one cycle of a sine, crushed (bits, rate) | All | crush (pack) |
| `@m2/effects` Widen | a stereo field from narrow to wide | All | field (pack) |
| std Lowpass, Highpass, Bandpass, Notch | the filter's real curve; one dot (cutoff and Q) | All | filter (std) |
| std Gain, Pan | knobs only (pan as its usual knob) | none | none |

### 5.3 Building blocks and the mix

| Component | Card |
|---|---|
| std `sine`, `saw`, `square`, `tri`, `noise`, `adsr`, `follow`, `svf-*`, `vca`, `add`, `scale`, `blend`, `sum`, `mtof`, `range`, `xrange`, `when`, `choose` | **small cards**, as today: knobs in a grid, no face, no tabs. They live in Patch, not in the Device chain (taste.md §3.5). |
| std LFO | a small card with its shape drawn (std picture), as `adsr` could be later |
| std Mixer | the compact card "Mixer · N channels · open Mix" (MX-6), unchanged |
| std Master | its ceiling over the level history (std picture) |
| a CLAP plugin | its parameters as today (CARD-6), in a grid, and its own window from the card; no pictures |

**Pictures needed, by where they live:**
- **std** (`std/pictures`, TypeScript, new): envelope, filter, transfer curve, level history, LFO shape, and EQ3's curve.
- **Packs:** EQ curve, multiband split, de-esser band, transient shape, Pump's dip, the delay's echoes, the reverb's tail, motion, the drive curve, crush, stereo field, wavetable frames, the synth's wave, the FM partials, the drum's hit.
- **Host** (music2's own, as today): waveform (a recording), keys, scope, and the live layers (spectrum, level).

## 6. What it costs, and in what order

### 6.1 What the core gains, counted

| Where | Gains | Why TypeScript alone can't do it |
|---|---|---|
| `@m2/core` (2.4.0, a minor: nothing removed) | `view.picture`; `axis.lin`, `axis.log`; `line`, `area`, `handle`, `mark`, `label`; `tap.spectrum`, `tap.level`, `tap.scope`: **one view kind and eight words** | The window must know a shape to draw it in its style, and a dot to give it a knob's gesture. The words are the contract between a pack and the window. |
| a descriptor | `editor.face`: **one field** | Which values are the face is the author's choice; nothing else says it |
| the server | a picture runner (one QuickJS sandbox per pack, kept alive, with a time and memory limit) and a `picture` part in `ViewNode.views` (its shapes and words). The preview answer carries the dragged picture's new shapes. **No new route.** | Pack code must run somewhere outside the window, and the build's sandbox is the one already trusted |
| taps | an ask's `kind`: `scope` (today), `spectrum` (4,096 frames, Hann window, about 120 bins at 1/12 of an octave, worked out on the server), `level` (peak and RMS). A point may be a call's input as well as its output. **Two kinds of tap, one field.** | A spectrum needs the sound as it plays (§2.7), and only the engine has it |
| kernels | **none** | Gain reduction is level in minus level out, plus makeup: two level taps, with no kernel change (D8) |
| the window | one shape renderer (SVG), and the card frame (face, tabs, sizes, the knob grid) | the window draws |
| std | `std/pictures` (TypeScript only); std 0.10.0 | the shared pictures belong in the toolbox (PKG-17) |

Option A would add about eight view kinds now, each Rust, React, a rule and tests, and one more for every new idea after that.

### 6.2 Build order

Each track has a builder, a skeptic and the builder again, as in [Typed sounds](../cards/design.md) §11. The sizes are rough.

| # | Track | Builds | Size |
|---|---|---|---|
| 1 | `frame` | The card frame from what descriptors already say: groups as tabs, All, three sizes, the knob grid, segmented menus, no ports in Device, the code line, keyboard; `editor.face` | medium: web, `view.rs` (`face`), a rule or two |
| 2 | `pictures` | `view.picture` and its words in `@m2/core` with their type checks; the build's checks; the server's picture runner and the `picture` view part; the window's shape renderer and dot gestures (on CARD-22's mechanism) | large (about track `views`) |
| 3 | `spectrum` | the `spectrum` and `level` taps at a call's input and output | medium |
| 4 | `dress` | `std/pictures`; the EQ, Compress, EQ3, Pump, Wavetable and Drum pictures; then the rest of §5's table, pack by pack (each a pack's minor) | medium, then small for each pack |
| 5 | `views-to-std` (if D5 is yes) | the envelope and filter views as std pictures; the window's `EnvelopeView` and `FilterView` go | small |

Tracks 1 and 3 can start at once. Track 2 needs 1's frame, and track 4 needs 2 and 3.

## 7. Decisions for the owner

| # | Question | Recommended |
|---|---|---|
| D1 | Should a card have a face (the picture that matters and a few knobs) and tabs for the rest, in place of one long list? | **Yes.** It is what Ableton, FabFilter and Vital do, and the mockup shows it. |
| D2 | How should packs design their cards? A: only from music2's list of pictures. B: music2 draws the card, and a pack may add pictures of its own, made of shapes. C: a pack ships its own web page. | **B.** Every card still looks like music2 and an agent can read every picture, but a pack can draw anything a picture needs. C puts a stranger's code in the window, which the owner refused on 2026-10-31 (§9). |
| D3 | Where should a pack's picture code run: on the server, in the sandbox where pack code already runs (B2), or in the window, walled off (B1)? | **On the server (B2).** No pack code in the window, as §9 says, and agents get the same shapes. The cost: a curve follows a drag a few milliseconds late. |
| D4 | Should the groups a sound already declares become its tabs, with an All tab on every card? | **Yes.** It is one concept, not two, and All keeps every value one click away. |
| D5 | Should the envelope and filter views become std pictures, so the window has one way to draw pictures? | **Yes, after track 2.** It is a smaller core. Keys and scope stay music2's. |
| D6 | The three sizes (folded, normal, expanded): kept by this window only, as folding is today, or saved with the song? | **This window only.** How big a card is doesn't change the music. |
| D7 | Keys for the card sizes? | **Later**, with taste.md's shortcut map. The buttons are in the tab order already. |
| D8 | Gain reduction: worked out from the level in and the level out (no kernel change), or sent by each compressor's kernel (exact, but a change to each)? | **From the levels**, now. A kernel's own meter can come later if the difference shows. |
| D9 | The Drum's face: draw the hit from the pack's maths, with the last real hit drawn over it as you play? | **Yes.** The maths draws it before you play. The real hit shows what you heard. |
| D10 | Should the Device chain drop the ports and the "In audio" rows, and leave cables to Patch? | **Yes.** Device is for playing the sound; Patch is for wiring it. |

## 8. The mockup

`$SCRATCH/cards2-note/mockup.html` is one file with no network and no storage. It works in light and dark and at phone width.
- It shows the Parametric EQ (drag the dots over a moving spectrum, scroll for Q), Compress (drag the threshold and the ratio on the curve, the level dot, the gain-reduction meter and the history), the Wavetable (tabs Osc A, Osc B, Filter, Env, LFO, More), the Drum (its hit drawn from the same numbers it plays, so a click sounds it) and Pump (its dip over a bar, with the playhead).
- Each card has the three sizes, an All tab, and the code line its changes write.
- Its sound is made up: there is no engine behind it.

![The mockup](images/mockup.png)
