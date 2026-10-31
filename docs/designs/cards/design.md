# Typed sounds: std a toolbox, packs the sounds, cards drawn from types

| | |
|---|---|
| **Status** | Accepted by the owner on 2026-10-31 ("Yes. Build out everything … simple yet elegant … a novel type safe DAW built from first principles"), with the recommendations in §10. |
| **Written** | 2026-10-31, from `main` at `774be44` |
| **Gaps** | #35 (knobs for more numbers), #20 (meters after the fader), #22 (the Sampler: a pedal, a turn for each key), #39 (a time limit on a kernel in the app) |

**In short**
- **A sound is a typed function, and its type is its interface.** `component(descriptor, body)` already
  says what a sound takes. Everything you can do to a sound comes from that one declaration: its card,
  the code editor's hints, `m2` and the API, presets and automation.
- **The type says what each value is, never how to draw it.** A number in a unit, a choice, an on/off, a
  recording. The app picks the knob, the menu, the switch or the waveform. So every card looks like
  music2, and an agent sees what you see.
- **std becomes a toolbox; the sounds become packs.** std keeps what sounds are made of: oscillators,
  filters, envelopes, the Sampler, the mixer. Every named sound moves to an `@m2/` pack that ships with
  the app and installs like anyone's.
- **A pack's code never runs in the window.** It runs in the build's sandbox, and its sound code
  (kernels) in WebAssembly, now with a time limit.
- **The pack contract is the platform.** Once other people write packs, `@m2/core` and the kernel ABI
  change only by version.

## 1. The problem

- A card shows knobs only. The SDK can already declare a menu (`setting.choice`), a switch
  (`setting.bool`), a whole number (`setting.int`) and a recording (`setting.asset`), but the card shows
  none of them (#35). std's LFO declares its shape as a menu; it can be changed only in code.
- Nothing but a number can change while a sound plays, or be automated: a setting rebuilds the sound.
- A card can't show a picture: an envelope's shape, a filter's curve, the Sampler's key map, the sound
  as it plays.
- The meters read each track before its fader (#20).
- std holds over 30 named sounds. Each new one would grow the app. The owner wants sounds from packs.
- A pack's kernel that never returns hangs playback (#39). Kernels get no events, so there is no sustain
  pedal and no turn for each key (#22).

## 2. First principles

1. **One declaration.** A sound's descriptor is written once, in TypeScript. The card, the editor's
   types, the API, presets and automation all read it. Nothing about a sound is said twice.
2. **What, not how.** A descriptor names the kind of each value. The app maps kinds to widgets. A pack
   can't add a widget: the vocabulary is closed and small, and adding a word to it is a change to the
   contract.
3. **Every value has one home** ([HOME-1](../../architecture.md#homes-of-values)). A change on a card
   writes the code, as a knob does today. A view holds no value: it shows, and moves, values that have
   homes.
4. **Settings build; controls play.** A setting decides what is built (which kernels, how many voices);
   changing one rebuilds that sound. A control is a value the built sound reads while it plays; it moves,
   and can be automated, with no rebuild. Either can be a menu.
5. **Checked twice.** TypeScript checks a descriptor, and every call of it, as you type. The build checks
   them again, since types can be bypassed (`as any`).
6. **The toolbox is small; sounds are packs.** std holds only what sounds are made of. music2's own
   sounds are packs like anyone's, with no special path.
7. **Looking costs nothing.** A tap copies a signal for the window. It never changes what plays, never
   runs in an export, and costs nothing while no one looks.

## 3. The vocabulary

### 3.1 Values

| Kind | Declared | Written in a song | Its TypeScript type | On the card | A change |
|---|---|---|---|---|---|
| Number | `control.hz(800)`, any unit | `cutoff: 800` | `number`, or a quantity | a knob | plays at once; can be automated |
| Choice | `control.choice(["saw", "square", "sine"], "saw")` **new** | `shape: "square"` | `"saw" \| "square" \| "sine"` | a menu | plays at once; can be automated |
| Switch | `control.bool(false)` **new** | `sub: true` | `boolean` | a switch | plays at once; can be automated |
| A number setting | `setting.int(8)`, `setting.number(1)` | `voices: 8` | `number` | a number box | rebuilds the sound |
| A choice setting | `setting.choice(["lane", "key"], "lane")` | `turns: "key"` | `"lane" \| "key"` | a menu | rebuilds the sound |
| A switch setting | `setting.bool(true)` | `mono: false` | `boolean` | a switch | rebuilds the sound |
| A recording | `setting.asset()` | `file: "kick.wav"` | `string` | its waveform; drop a file on it to replace it | rebuilds the sound |
| Regions, a keymap | `setting.regions()`, `setting.keymap()` | (a list) | `Region[]`, a record | a key map (§3.2) | rebuilds the sound |
| A track | `setting.key()` | `to: "drums"` | `string` | a menu of the song's tracks | rebuilds the sound |

- **A choice is its name everywhere a person or a file sees it.** The code writes `"square"`, a lane's
  points hold `"square"`, a preset holds `"square"`. A kernel gets the option's place, 0 to n − 1, as a
  number. So a pack's new version may add or reorder options and no song changes.
- **A switch** reaches a kernel as 0 or 1.
- **A lane of a choice or a switch steps:** it holds each value until the next point. A point naming an
  option the sound no longer has is a diagnostic that names it, and plays the default there.
- A choice signal can't be wired into a number input of another unit, and a number can't be wired into
  a choice: the checker refuses it, as it refuses Hz into dB.

### 3.2 Views

A view is a picture over values the sound declares. It holds no value of its own.

| View | Declared | Shows | You can |
|---|---|---|---|
| Envelope | `view.envelope("attack", "decay", "sustain", "release")` | the envelope's shape | drag its corners, which moves the four controls |
| Filter | `view.filter("cutoff", "q")` | the filter's curve | drag its peak: cutoff and q |
| Keys | `view.keys("regions")` | which recording plays on which keys and how hard | look, and click a key to hear it |
| Scope | `view.scope()` | the sound's output as it plays (a tap, §5) | look |

A recording (`setting.asset`) is always drawn as its waveform; it needs no view.

**Names are checked by type.** `view.envelope("attack", "decay", "sustain", "release")` must name
controls of this sound: attack, decay and release in a time unit, sustain in a level unit. A typo, or a
cutoff where a time goes, is a red line in the editor, and the build refuses it in words.

### 3.3 Groups

`editor.groups` puts a card in order:

```ts
editor: { category: "Instruments", groups: { Oscillator: ["shape", "table", "sub"], Filter: ["filter", "cutoff", "q"], Amp: ["amp"] } }
```

Each entry is a value or a view of this sound, checked by type, and named once in all the groups. A
value in no group goes in a last group, More. A folded card shows its first group. With no groups the
card is as today: every value in the order it is declared.

### 3.4 Plugins

A CLAP plugin's parameters map onto the same kinds:

| A parameter that is | Becomes |
|---|---|
| stepped, 2 to 64 steps, the plugin naming each step differently | a choice: a menu of the plugin's names |
| stepped from 0 to 1 | a switch |
| anything else | a number: a knob |

The code still holds the number ([FMT-18](../../architecture.md#plugin-state-and-parameters),
[HOME-4](../../architecture.md#homes-of-values)); the menu shows the plugin's name for it.

### 3.5 An example

```ts
import { component, control, setting, port, view } from "@m2/core";

export const wobble = component({
  id: "wobble", label: "Wobble Bass",
  inputs: { notes: port.notes() },
  controls: {
    shape: control.choice(["saw", "square", "sine"], "saw"),
    cutoff: control.hz(800).range(40, 8000).log(),
    q: control.scalar(0.7),
    attack: control.secs(0.005), decay: control.secs(0.2), sustain: control.frac(0.6), release: control.secs(0.3),
  },
  settings: { sub: setting.bool(true), voices: setting.int(8) },
  output: port.audio(),
  editor: {
    category: "Instruments",
    views: { amp: view.envelope("attack", "decay", "sustain", "release"), filter: view.filter("cutoff", "q") },
    groups: { Sound: ["shape", "sub"], Filter: ["filter", "cutoff", "q"], Amp: ["amp"] },
  },
}, (notes, c) => …);
```

In a song, `wobble(lane.notes("bass"), { shape: "square" })`, and the code editor offers
`"saw" | "square" | "sine"` as you type. That union of names is TypeScript's own menu.

## 4. Changing a value on a card

- A card's menu, switch, number box or dropped file is **one Tx that writes the literal into the call**,
  by the rules a knob's first edit follows ([PKG-3](../../architecture.md#component-descriptors)'s
  insertion): one Undo step, checked against the graph after. A value the code works out (a variable,
  `2 * 440`) shows read-only, with **Edit in code**.
- A setting's change rebuilds that sound, so a note held at that moment may stop; the card says so when
  you point at it. A control's change doesn't.
- A file dropped on a recording is brought in as Import Audio brings one in
  ([FMT-14](../../architecture.md#assets-and-hashes)), then written.
- **Automate** on a menu or a switch is [HOME-2](../../architecture.md#homes-of-values)'s Tx, with a lane
  of names, or of true and false.
- The API and `m2` see the same: each value in `GET /api/graph` carries its kind, its options and its
  home, and one route writes any of them.

## 5. Taps: looking without touching

- A **tap** copies a signal at one point of the graph, after the block is made, into a buffer the server
  reads: a meter's peak and loudness, or a scope's last frames.
- **A tap changes no bit.** The graph never reads one, and an export has none: the bit-identity tests
  ([RENDER-33](../../architecture.md#the-bit-identity-test)) hold with taps on.
- A tap runs only while a window asks for it: a card's scope open, the Mix view showing. The server sends
  what changed about 30 times a second over the WebSocket. The audio thread never waits for, or
  allocates for, a tap.
- **Meters after the fader** (#20) are taps on each channel after its gain and pan. The master's tap
  also gives the loudness of the last 3 seconds (LUFS) and the true peak, worked out off the audio
  thread, as `m2 listen` measures them.

## 6. Sound code that can't hang, and that hears events

### 6.1 A time limit

Every kernel call runs with a deadline (wasmtime's epoch interruption: a counter that another thread
moves on; a call past its deadline stops with a trap).
- **Live**, a call that runs far past a block's time stops. That sound goes silent until the song builds
  again, and the status names it and its pack ("Growl, from @alice/growl, took too long and was stopped").
- **In an export** the limit is generous (one call may take seconds). Past it the export fails in words;
  it is never quietly different.
- The checks cost a little in every kernel. That cost is measured with the timing gates before it ships.

### 6.2 Events for kernels

ABI-2 reserved event ports; they are built now, in the shape reserved (`{count, cap, events}`,
[RENDER-14](../../architecture.md#events)'s 24-byte `Event`), as an addition that keeps kernel ABI v1
([ABI-3](../../architecture.md#kernel-abi-reservations)). A kernel that declares an `events<note>` input
gets its lane's notes and controller messages each block, stamped to the sample. The first use: **a turn
for each key**. std's Sampler counts round robin for each key (`turns: "key"`, a choice setting), as SFZ
does. `"lane"` stays the default, so no song changes sound; `m2 pack import-sfz` writes `"key"`.

### 6.3 The sustain pedal

Sustain belongs to the notes, not to each sound. While the pedal is down (CC 64 at 64 or more), a std
instrument's voice keeps its note on until the pedal goes up: the voices
([RENDER-16](../../architecture.md#voices)) hold it, so every std instrument, and every pack instrument,
gets the pedal at once. A plugin gets CC 64 itself, and holds its own notes. For now the pedal is live
only: a notes lane can't record it yet (#24, which needs a format change).

## 7. std as a toolbox

| std keeps (what sounds are made of) | Moves to a pack (the sounds) |
|---|---|
| `std/dsp`, `std/filter`, `std/math`, `std/control`: oscillators, filters, envelopes, LFOs, math | `@m2/synths`: `std/instruments`' five, `std/synth`'s and `std/fm`'s named sounds and their custom engines, with their presets |
| `std/sampler`: the Sampler | `@m2/drums`: the kit and every drum |
| `std/mix`: the mixer, the buses, the master | `@m2/effects`: delay, reverb, chorus, phaser, tremolo, drive, bitcrush, widen |
| the effects mixing needs: `eq`, `compress`, `limit`, `duck` | |
| every kernel (`std.wasm`) | |

- **The kernels stay in std.** A pack's sound calls them by id (`kernel("std:svf", …)`), as any pack can.
  Moving a pack's kernels into a wasm of its own is later, and changes no sound.
- **The move changes no bit.** Each fixture renders the same bits through the packs as it did through
  std (`renders.json`, `e2e/renders.sha256`).
- The packs live in the repo at `packs/@m2/<name>/`, are checked by `m2 pack check` in CI, and ship
  inside the app. **When the app (or `m2`) starts, it installs each bundled version the store hasn't
  got** ([PACK-6](../../architecture.md#packs): installing a version that is there with its hash does
  nothing). So there is one store and one path: a song locks `@m2/synths` 1.0.0 as it locks any pack,
  and keeps it after the app moves to 1.1.0. The same versions go to the registry.
- Templates and examples list them in `[dependencies]` and lock them. + Track, ⌘F and the browser show
  them as installed packs, with no special case.
- std's version moves to 0.6.0, since it loses components. A song asking for std "0.5" is told what to
  change, as today ([FMT-3](../../architecture.md#manifest-and-versions)).

## 8. The pack contract

- The pack contract is `@m2/core` (the descriptor API, §3) and kernel ABI v1 (with §6.2's events).
  Once wave 3 lands (§11), `@m2/core` is 2.0.0, and a change that breaks a pack's code is a new major.
  [PACK-2](../../architecture.md#packs)'s caret rule already refuses a pack made for another major, in
  words.
- Songs keep "no backwards compatibility before 1.0". Packs get this one promise, because other people's
  work depends on it.
- CI builds every pack the repo ships, and the registry's robot builds every listed version.

## 9. Alternatives

| Instead | Why not |
|---|---|
| Packs bring their own window code (HTML and JavaScript) | Code from strangers in the window; every pack looks different; an agent can't see in. CLAP plugins already do this, in their own windows. |
| One control kind with a `widget` field | It says how, not what (§2.2), and a widget would need its own checks |
| Choices as numbers in code (`shape: 2`) | Unreadable, and a pack's new version reordering its options would change songs |
| Sustain inside each kernel | Every instrument would have to do it, and most wouldn't |
| Fuel for the time limit (counting instructions) | Exact, but slower; exactness isn't needed to stop a hang |
| Bundled packs read from inside the app | A song locked to an older version would lose it when the app updates |

## 10. Decisions

| # | Question | Decided |
|---|---|---|
| D1 | std a toolbox, with its sounds in bundled `@m2/` packs? | Yes (the owner, 2026-10-31) |
| D2 | Cards drawn from types, with no pack code in the window? | Yes (the owner) |
| D3 | The pack contract stable by version? | Yes, from wave 3 (the owner, as recommended) |
| D4 | A choice in files: its name or its place? | Its name; a kernel gets its place |
| D5 | Sustain: in each kernel, or in the voices? | In the voices; a plugin gets CC 64 |
| D6 | The time limit: fuel or epochs? | Epochs |
| D7 | Bundled packs: read from the app, or installed into the store? | Installed into the store |
| D8 | Which effects std keeps | The ones mixing needs: eq, compress, limit, duck |

## 11. Plan

Each track has a builder, a skeptic who attacks what it built, and the builder again to answer every
finding. Then `ci/check.sh --pr`, and it lands. Rule ids are given out ahead, so tracks never clash.

**Wave 1**, in parallel:

| Track | Builds | Rules | Gaps |
|---|---|---|---|
| `cards` | §3.1's settings on the card, §3.4's plugin menus and switches, §4's writes | CARD-1 to CARD-9 | #35 |
| `kinds` | §3.1's choice and switch controls, §3.2's views and §3.3's groups in `@m2/core`, with their types and the build's checks; lanes of names; each value's kind in the API | CARD-10 to CARD-19 | |
| `taps` | §5: taps, meters after the fader, loudness, a scope's stream | TAP-1 to TAP-9 | #20 |
| `limits` | §6.1 | ABI-6, ABI-7 | #39's kernel limit |
| `events` | §6.2 and §6.3 | ABI-8, ABI-9, RENDER-35 to RENDER-39 | #22's pedal and turns |

**Wave 2**, in parallel: `starter` (§7; PACK-15 to PACK-19, PKG-17) and `views` (the card's menus and
switches for controls, their lanes on the timeline, groups, the four views; CARD-20 to CARD-29).

**Wave 3:** the starter packs dressed with groups, views and choices, and `@m2/core` 2.0.0 (§8).
