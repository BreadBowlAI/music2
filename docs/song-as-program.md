# Song as a program

The design of music2.

Status: design (2026-10-11), partly built. What exists is listed under [Implementation status](#implementation-status); anything marked **Not built yet** is design only.

## Summary

**A song is a program whose only input is time.**

- **What a song is made of:**
  - **Graphs** describe how it sounds. The TypeScript code is the source of truth, and the node editor is a live, two-way view of it, the way Xcode's canvas relates to SwiftUI code: drag a knob or a cable and the code changes.
  - **Timeline data** is what gets played: notes, automation, masks, audio clips and tempo.
  - The graph is the source of truth for behaviour. Per-clip behaviour comes from masks, never from per-clip changes to the graph.
- **How it runs:** a compiler checks the whole song, and a small Rust core plays it.
- **The core contains no instruments, effects or DSP.** Oscillators, filters, synths, mixers, LFOs and arrangement helpers all live in a **standard library** that ships the same way as any community package. The core only:
  - checks types
  - schedules and runs graphs
  - hosts sandboxed DSP code
  - stores the project
- **Professional from the start:** audio clips (warping, resampling, comping) are as first-class as notes, and the plugins producers already use (CLAP, VST3, AU) run as ordinary nodes beside the open WebAssembly ones.
- **Who uses it:** artists build songs from packages that others publish. AI agents work on the same project through the same API as the GUI, but they never get the final say.

## Principles

1. **The minimal-core rule.** Something belongs in the core only if it can't be a package, or if making it a package would break a guarantee: safety, determinism, or a single source of truth. Everything else is a package.
2. **No privileged packages.** The standard library uses the same plugin interface as everyone else. If the standard library can't be built as an ordinary package, the plugin system is missing something, and that gets fixed in the core.
3. **Time is the only invariant.** Every signal is a function of song time. Kernels (compiled DSP code) see time only through their input ports.
4. **Deterministic wherever it can be.** WebAssembly kernels give the same audio, bit for bit, on every machine, which makes caching, testing and reproducibility work. Hosted plugins (CLAP/VST3/AU) can't promise that, so they are marked impure: never cached, and reported as such. A song using only open kernels is fully reproducible.
5. **Files are the truth.** A project is a folder of text and assets, and it diffs in git. The engine is the only writer at runtime, and every client goes through it. Today other editors and agents also write the files; the app picks the change up within 300 ms.
6. **Behaviour, data and layout are kept apart:**
   - Code is behaviour.
   - The timeline, audio and plugin state are data.
   - `layout.json` only controls how things are drawn.
7. **Code is the source of truth; the graph is a view you can edit.** Every edit in the node editor becomes a small, checked change to the code (see [Editing code from the graph](#editing-code-from-the-graph)). Turning a knob is instant; the new value is written into the code when you let go.
8. **Agents are clients, not features.** There is no AI built into the app. Agents get the same commands as the GUI, plus a way to "hear" the song, plus variants they can propose.
9. **The artist decides.** Agents propose on variants, and locks protect what the artist doesn't want touched.

## Layers

```
┌──────────────────────────────────────────────────────────────────────┐
│ Clients     artist (GUI) · Claude Code / any MCP agent · scripts      │
├──────────────────────────────────────────────────────────────────────┤
│ Hosts       desktop app (timeline, node editor, mixer view) · CLI ·  │
│             server (HTTP/WS/MCP) · analysis tool · LSP/type gen       │
├──────────────────────────────────────────────────────────────────────┤
│ Packages    std/* (standard library) · github:anyone/anything         │
│             TypeScript graph modules + WebAssembly kernels + tools    │
├──────────────────────────────────────────────────────────────────────┤
│ Core        types & graph IR · planner · runtime + kernel host ·      │
│             script sandbox · project, transactions & undo ·           │
│             package resolution · render                               │
└──────────────────────────────────────────────────────────────────────┘
```

---

## The model

### Time

- **Song time** is measured in beats.
- The **tempo map** converts beats to seconds. It is **timeline data**, not code, so time can never depend on the graph.
- The core gives every module two read-only inputs: `time.beats` (`Control<Beats>`) and `time.seconds`.
- Derived clocks are meant to be ordinary standard-library functions of `time.beats`, such as `every("1/8")`, `swing(0.6)`, `div(3)` and `phase(...)`, so polyrhythm and swing need nothing special in the core. (They belong in `std/time`, which is **Not built yet**.)

### Signals

There are three kinds of signal, and every connection has exactly one kind:

| Kind | Meaning | Rate |
|---|---|---|
| `Audio<Ch>` | sound, mono or stereo | sample rate |
| `Control<U>` | a continuous value with a unit | sample rate (computed for every sample) |
| `Events<T>` | discrete things at an instant or over a span | exact to the sample |

**Units** are part of the type. The core defines a fixed set: `hz`, `db`, `gain` (linear), `semitones`, `seconds`, `beats`, `unit01` (0..1), `bipolar` (-1..1) and `scalar` (unitless).
- Connecting signals with mismatched units is a compile error.
- A few safe widenings are allowed: `unit01` fits where `gain` or `scalar` is expected, and `bipolar` and `gain` fit `scalar`.
- In TypeScript a unit is attached with a function, since JavaScript numbers can't carry one: `hz(800)`, `db(-6)`, `ms(30)`. A plain number is an **untyped literal** that fits any unit.
- Conversions are standard-library kernels.
- Kernel ports can be **generic**: `*` (any kind), `audio<*>` (any channel count) and `control<*>` (any unit). All of a kernel's `*` ports share one type variable, so `add(a: *, b: *) -> *` adds two audio signals or two controls of the same unit, but never one of each. A mono signal feeding a stereo port is copied to both channels.

**Masks** are `Control<Unit01>`, and **gates** are masks that are only ever 0 or 1. Masks are how per-clip and per-section behaviour is expressed without changing the graph.

The **note** type is defined by the core, because both the timeline and `poly` depend on it:

```ts
type Note = {
  pitch: Control<Semitones>;    // includes per-note bends (MPE)
  velocity: Control<Unit01>;    // 0..1
  gate: Control<Unit01>;        // 1 while held
  pressure: Control<Unit01>;
  timbre: Control<Unit01>;
};
```

### Nodes

A node is one of three things:
- a **kernel instance**: open, sandboxed WebAssembly DSP from a package
- a **plugin instance**: a CLAP, VST3 or AU plugin, with its saved state stored as data (see [Hosted plugins](#hosted-plugins))
- a **module instance**: a graph from a package or from your project

Every node is:
- **Causal.** Its output at time `t` depends only on its inputs up to `t`, plus any **look-ahead** it declares. (Declaring look-ahead is **Not built yet**.)
- **Deterministic** (kernels). Kernels run in a sandbox with no access to a clock, randomness, threads or I/O. Random numbers come from seeds passed in as inputs. Plugins are the exception and are marked impure.
- **Explicit about latency.** Every kernel declares its latency, and the planner compensates automatically. (Compensation is **Not built yet**: nonzero latency is refused.)

### Lanes and clips

A **lane** is a typed input of a module that the timeline fills in.

| Lane | Signal | What the clips contain |
|---|---|---|
| `lane.notes(id)` | `Events<Note>` | notes |
| `lane.control(id, unit, default)` | `Control<U>` | breakpoints, with straight lines in between (curves are **Not built yet**) |
| `lane.mask(id)` | `Control<Unit01>` | breakpoints, default 0 |
| `lane.audio(id)` | `Audio` | audio clips (see [Audio clips](#audio-clips)) |

The song itself also has lanes for **tempo** and **sections**. Sections are arrangement markers such as `intro` and `chorus`, stored as `Events<Section>`. (Sections are **Not built yet**; the tempo lives in `timeline/song.json`.)

**Clips are always data and never code.** Where automation or mask clips overlap, the one that starts later wins. Overlapping notes clips all play, and overlapping audio clips are mixed together. In gaps, the lane's default value applies.

Per-clip behaviour has two forms, both data:
- **Clip properties:** gain, fades, reverse, pitch, a start offset into the source, warp markers. The lane applies them as it plays the clip.
- **Masks:** 0..1 control lanes that switch graph behaviour on and off over time.

Behaviour that needs its own effect chain (one chop through its own delay) goes on its own lane or track. The graph itself never changes per clip.

### Audio clips

Audio is the centre of a lot of modern production, so audio clips are first-class:
- **A clip** refers to an asset (content-addressed, in `assets/`), with a start offset, a length, gain, fades, reverse, and a pitch shift in semitones. (Today assets are named files, not content-addressed.)
- **Warping:** warp markers pin positions in the source to beats. Between markers, the audio is time-stretched to follow the tempo map. Stretching is a package kernel (std ships one), applied offline and cached, so playback stays cheap and deterministic. (Today stretching is in the core runtime; see [Implementation status](#implementation-status).)
- **Takes and comping** (**Not built yet**): a recording pass adds takes to a lane. A comp is clip data that picks regions from takes.
- **Bounce and resample:** any signal over any time range can be rendered to a new asset and dropped onto an audio lane, to chop it, reverse it and process it again. Because rendering is deterministic, a bounce of open kernels can always be reproduced. This loop (render, chop, render again) is central to sound design, and it is data all the way down.

### Modules and tracks

A **module** is a graph with lanes and named **exports**. It is the unit of reuse.

A **track** is a module that the song lists in its arrangement, so it gets timeline rows for its lanes. The core only knows about modules. "Track" is a convention used by the SDK and the hosts.

```ts
export default track("Bass", ({ lane, time }) => {
  ...
  return { out, env };            // exports
});
```

### Polyphony

```ts
poly(notes, n => voice(n), { voices: 16 })
```

`poly` creates one instance of the voice graph for each sounding note and sums the outputs. A voice ends once its note is over, its output is quiet, and no kernel in it is still active (an envelope releasing, echoes in a delay). `poly` is in the core because it changes the shape of the graph while it runs, which no package can do.

### References between modules

**The graph only pulls.** A consumer imports a producer's export: `duck(bass.out, kick.env)`. Nothing pushes.

- The planner sorts the whole flattened song into a processing order.
- **A cycle is a compile error**, and the error names the loop.
- Intended feedback must go through `feedback(kind)`, whose input is given later with `.set(signal)`. It delays by one processing block (128 samples).
- A "send" in the GUI is a view of an edge that the bus pulls in.

---

## The core

This is the whole core. If something isn't listed here, it lives in a package or a host.

### 1. Graph IR: eight ops

The IR is the compiled form of a song: a JSON graph of nodes. Every graph, however it was authored, compiles down to these eight ops:

| Op | Meaning |
|---|---|
| `lane` | a module's typed timeline input |
| `time` | `beats` / `seconds` |
| `const` | a typed constant (a number with a unit, or an asset reference) |
| `kernel` | an instance of a package kernel, with its port connections |
| `poly` | maps a voice subgraph over `Events<Note>` and sums the results |
| `feedback` | an explicitly delayed edge, the only way to form a loop |
| `import` | another module's export |
| `plugin` | a hosted CLAP/VST3/AU plugin instance: its ports, its state (an asset), its reported latency; impure |

Each module also lists its `exports`. The **validator** checks the IR independently of whatever produced it:
- signal kinds and units
- ports exist and are connected
- no cycles
- rates and latency are consistent (today any nonzero latency is refused; look-ahead is **Not built yet**)

### 2. Planner

The planner turns validated IR into a plan the runtime can execute:
- flatten modules into one graph
- sort into a processing order
- insert latency compensation (**Not built yet**)
- assign buffers
- prune anything that doesn't reach the root output
- assign each node a **stable id** derived from its position in the source (overridable with `.id()`) (**Not built yet**: ids are counters)

### 3. Runtime and kernel host

- Processes audio in blocks, either in real time to the audio device or offline as fast as possible.
- **Hot swap:** a new plan is compiled on a worker thread and handed to the audio thread without locking. Nodes whose stable ids match carry their state over, so reverb tails don't cut off and held notes keep sounding. (Carrying state over is **Not built yet**: today a new renderer is built and replayed up to the playhead.)
- **The kernel ABI: WebAssembly**, run with wasmtime (Cranelift). Relaxed SIMD is disabled, so output is bit-identical on every machine.
  - NaN bit patterns are the one thing WebAssembly doesn't pin down. Canonicalising them cost 4.6× in the M1 benchmark, so the runtime doesn't. Instead it silences any non-finite output sample and reports it, because a NaN means a kernel is broken anyway.
  - A package ships one `.wasm` module that can hold many kernels, and every graph node gets **its own instance**, so a kernel's state is just its instance's memory. For a kernel named `k`, the module exports:
    - `k_init(sample_rate, max_block, channels)`
    - `k_ports()`: the number of inputs and outputs, which the host checks against the manifest
    - `k_port(i)`: the address of port buffer `i` (inputs first, then outputs; channel-major, `2 × max_block` floats)
    - `k_reset()`
    - `k_process(frames)`
    - `k_active()`: whether sound the kernel has already taken in can still come out later (echoes in a delay line, an envelope's release). Brief filter ringing and sound made from nothing (an oscillator) don't count. A `poly` voice ends only when its note is over, its output is quiet, and no kernel in it is active.
    - plus `m2_abi()` (checked against the runtime's ABI version at load) and `memory`, once per module
  - **No host imports.** A module that imports anything is rejected at load, so a kernel can't reach anything outside its own ports.
  - Each instance's memory is capped (32 MB). A CPU budget is still to come.
  - Rust kernels use the `m2-kernel` crate: implement `Kernel` and call `export!(name, Type)`.
  - Assets (for samplers and impulse responses) are passed in as read-only data when the kernel starts. (**Not built yet.**)
- **The realtime model.** A professional session has 50–150 tracks and hundreds of plugin instances, so:
  - **Parallel** (**Not built yet**). The planner partitions the graph, roughly one partition per track plus buses, and schedules partitions on a thread pool as their inputs complete. Each partition owns its own WebAssembly store, so partitions never share mutable state.
  - **Realtime-safe.** Nothing on the audio thread allocates, locks or waits on the compiler. Voice pools, buffers and plans are built off-thread and handed over.
  - **A fast path for values.** Changing a constant (a knob) updates the running plan in place without recompiling. Only structural changes (adding, removing or rewiring nodes) build a new plan, which is swapped in at a block boundary.
  - **Latency compensation** (**Not built yet**) is required once plugins are hosted: plugins report latency, and the planner delays parallel paths to match.
  - **Monitoring** (**Not built yet**). Live input (recording, playing an instrument) takes a low-latency path that skips caches.
- **Render and taps.** `render(range, taps)` renders a time range and returns audio, plus the values of any signal you ask for (for example, a meter kernel's output). Rendered results are **cached by a hash of what produced them** and invalidated only downstream of an edit. Freezing a track just means keeping its cache. (Rendering a range is built; taps and caching are **Not built yet**.)
- **The audio device:** output, input and MIDI in. This is the one place the core touches hardware. (Output is built, in `core/play`; input and MIDI in are **Not built yet**.)

### Hosted plugins

Producers rely on plugins that will never be rewritten as WebAssembly, so hosting them is part of the core runtime, not an optional extra:
- **CLAP first** (open standard, clean threading and state model), then VST3, then AU on macOS. (CLAP is built; VST3 and AU are **Not built yet**.)
- A plugin is a graph node with typed ports: audio in and out, notes in for instruments, and its parameters exposed as control inputs, so they can be automated and connected like any other input.
- **Its state is data.** The plugin's saved state is an asset referenced from code (`state: "bass-space"`), so the code stays readable and the state diffs as a file.
- **Out of process, optionally** (**Not built yet**). Plugins can run in a separate host process so a crash can't take down the session (at a small latency cost). This can be chosen per plugin.
- **Impure:** never cached, never assumed reproducible. A render that includes them should say so. (`m2 bounce` does, in its output and its provenance file; `m2 render` doesn't yet.)

### 4. Script sandbox

- Strips TypeScript types (oxc) and runs the builder in an embedded JavaScript engine (QuickJS).
- The sandbox has no file access, no network and no clock, and randomness only comes from seeds. Running a module can only **describe a graph**.
- That sandbox is what keeps code packages as safe to install as data.
- The `@m2/core` TypeScript SDK is part of the core. It is small: the signal and unit types, `track` and `song`, `lane`, `poly`, `feedback`, `kernel`, `plugin`, `component`, and graph-handle recording.
- Typed bindings for kernels will be **generated from package manifests** (**Not built yet**). In M1, std's wrappers (`std/dsp` and the rest) are written by hand over `kernel("std:svf", {...})`. Running `tsc` is an optional extra check for editors; it is never what decides whether a song compiles.

### 5. Project, transactions and undo

- Reads and writes the project folder and watches it for edits made from outside.
- Every change is a **transaction**:
  1. Apply the ops.
  2. Recompile.
  3. Validate.
  4. Commit, or reject with nothing changed.
- When a transaction fails, it returns **structured diagnostics** that point at both the source location and the graph node, so an agent can fix its own mistake.
- **Undo** works per transaction, whether the edits were to code, data or both.
- **Locks** (**Not built yet**; in the manifest) are enforced here, because the core is the only writer:

  ```toml
  [locks]
  "tracks/vox.ts" = "graph"      # graph frozen, lanes editable
  "timeline/vox.*" = "all"
  ```

### 6. Package resolution

**Not built yet.** Today only `std` is loaded: from `$M2_STD`, from `music2.app`'s `Contents/Resources/std` (the app bundles it, with the templates and the examples), or from the source tree (`crates/server/src/resources.rs`).

- Resolves `music2.toml` dependencies, fetches packages (git sources such as `github:owner/repo@tag`, or bundled ones), verifies their hashes, and writes `music2.lock`.
- Checks that kernel manifests are compatible with the ABI version.
- The standard library is resolved exactly like any other package. The app ships a copy so it works offline, but the song's lockfile pins its version independently of the app.

### 7. The core API

These are the only commands the core provides. Hosts expose them over HTTP/WS and MCP, and the CLI calls them directly.

**Not built yet.** Today the session (`core/session`) offers specific edits (set a value, connect, disconnect, add or remove an effect, write a notes lane, save a file, undo, redo), and the app's server exposes them as the `/api/graph` routes listed under [The canvas today](#the-canvas-today). `m2 check` and `m2 render` are the CLI's versions of `check` and `render`.

| Command | Purpose |
|---|---|
| `project.get` | files, compiled IR, diagnostics and revision, or a compact `describe` summary |
| `tx` | an atomic batch of ops, compiled once and undone as one step |
| `undo` / `redo` | |
| `check` | compile without committing; returns diagnostics |
| `render` | a time range and taps, producing a WAV and tapped signals |
| `transport` | play, stop, seek, loop, live input monitoring |
| `subscribe` | patches whenever the revision changes (not built yet; today clients get a graph event and fetch again) |

The ops a `tx` can contain (**Not built yet**):
- **Files:** `file.write`, `file.patch` (text edits)
- **Timeline:** `clip.put`, `clip.delete`, `events.put`, `events.delete`, `points.put`, `points.delete`
- **Packages:** `pkg.add`, `pkg.remove`

Higher-level edits are **tools** (**Not built yet**): pure TypeScript functions from packages that take data and return `tx` ops. Examples include quantize, transpose, humanize, the node editor's code edits, and generators. The core's command set never grows.

### Not in the core

These live elsewhere on purpose:
- Every oscillator, filter, envelope, LFO, effect, instrument and the mixer (packages)
- Derived clocks, sections-as-masks, mute and solo (packages)
- Meters and analysis (meters are package kernels; spectrograms and reports come from a host tool)
- The GUI, node editor, piano roll, MCP server and variants (hosts)
- Plugin *packages*. Hosting CLAP/VST3/AU plugins is in the core runtime (see [Hosted plugins](#hosted-plugins)); the plugins themselves are installed by the user like in any DAW.

---

## The standard library

`std` is a set of ordinary packages, versioned separately from the app. Today it is one package in `std/`: one WebAssembly module with 29 kernels (`std/kernels/std.wasm`, built from `std/kernels-src`) and the TypeScript modules below (`std/src`).

**Built:**

| Module | Contents |
|---|---|
| `std/dsp` | `osc.sine`, `osc.saw`, `osc.square`, `osc.tri`, `noise`, `adsr`, `follow`, `svf` (lowpass, bandpass, highpass), `vca`, `gain`, `pan` |
| `std/math` | `add`, `scale`, `blend`, `sum`, `mtof`, `range`, `xrange` |
| `std/control` | `lfo`, `when`, `choose` |
| `std/filter` | `lowpass`, `highpass`, `bandpass`, `notch` |
| `std/fx` | `duck`, `delay`, `drive`, `limit`, `reverb`, `chorus`, `phaser`, `tremolo`, `compress`, `eq3`, `bitcrush`, `widen` |
| `std/mix` | `mixer` (channel gain and pan), `master` (a limiter) |
| `std/instruments` | `bass`, `kick`, `hat`, `pluck`, `pad` |

Seventeen of these are offered by the editor as effects (`[[effect]]` in `std/m2pkg.toml`): the four filters, the `std/fx` effects except `duck`, and `gain` and `pan`.

**Not built yet:**

| Module | Contents | Kind |
|---|---|---|
| `std/time` | `every`, `swing`, `div`, `phase`, `ramp` | kernels |
| `std/arrange` | `section(name)` → gate, `mute`, `solo`, `fadeIn`, `fadeOut` | modules |
| `std/meters` | `peak`, `rms`, `lufs`, `spectrum`, `onset`, `pitch` | kernels (used through `render` taps) |
| `std/gen` | `euclid`, `arp`, `chords`, `scale`, `strum` | tools (generators that output lane data) |
| `std/edit` | `quantize`, `transpose`, `humanize`, `legato`, `bake` | tools |
| `std/theory` | keys, scales, chord names | plain TypeScript |
| `std/instruments` | `lead`, `keys`, `strings`, `drums`, `sampler` | modules |
| `std/fx` | `pump` (ducking in time with the beat, with no sidechain) | modules |

Also not built: unison and FM oscillators, mute, solo, sends and buses in `std/mix`.

The standard library is also the main test of principle 2. If writing `std/mix` or `std/instruments/drums` needs something kernels and modules can't express, the core gets a general mechanism for it, never a special case.

---

## Packages and the ecosystem

### What a package contains

```
stutter/
  m2pkg.toml
  src/index.ts          # modules (graphs) and tools
  kernels/stutter.wasm  # compiled kernels (source in kernels-src/, any language)
  examples/demo/        # a tiny project using it
  README.md
```

Design; the real one is `std/m2pkg.toml`:

```toml
[package]
name = "github:someone/stutter"
version = "1.2.0"
license = "MIT"
description = "Beat-synced stutter and repeat"
tags = ["glitch", "stutter", "repeat"]

[[kernel]]
name = "stutter"
wasm = "kernels/stutter.wasm"
latency = 0
inputs  = { in = "audio<2>", beats = "control<beats>", rate = "control<beats>", mix = "control<unit01>" }
outputs = { out = "audio<2>" }

[modules]
index = "src/index.ts"
```

The real format is a little different. The `wasm` file is named once for the whole package, ports are `"name: kind = default"` strings, `[[effect]]` entries list what the editor offers as effects, and modules are found as `src/<name>.ts` (imported as `std/<name>`):

```toml
[package]
name = "std"
version = "0.1.0"
wasm = "kernels/std.wasm"

[[kernel]]
name = "sine"
description = "Sine oscillator"
inputs = ["freq: control<hz> = 440"]
outputs = ["out: audio<1>"]

[[effect]]
module = "std/filter"
name = "lowpass"
category = "Filter"
description = "Darker: removes the highs above the cutoff"
args = "1000"
```

### Three levels of trust

| Level | What | Safety | Reproducible |
|---|---|---|---|
| Graph modules and tools | TypeScript run in the sandbox | describe graphs only | yes |
| Kernels | WebAssembly with no host imports | memory- and CPU-bounded | yes, bit-identical |
| Hosted plugins (CLAP/VST3/AU) | the plugins producers already use | full trust, like any DAW; optionally out of process | no: marked impure, never cached |

### Versioning and lockfile

**Not built yet.**

- **A package's ports and parameters are its API.** Changing their types is a breaking change and requires a major version bump.
- On upgrade, the compiler checks your song against the new signatures.
- `music2.lock` pins every package by **content hash**, so a song still renders identically years later.

### Making packages easy to find, for people and agents

**Not built yet.**

Every package ships:
- a description and tags
- typed ports with units, and parameter ranges
- a runnable example
- **reference features** computed from its example render: brightness, transient sharpness, stereo width, loudness

Search uses all of this, so `pkg.search "dark wobbly bass"` can be answered from features as well as from tags.

---

## Project layout

```
my-song/
  music2.toml          # name, dependencies, locks
  music2.lock          # pinned package hashes
  song.ts              # root module: the final mix
  tracks/
    kick.ts
    bass.ts
  lib/                 # your own modules and tools
  timeline/
    song.json          # tempo map, time signatures, sections
    bass.notes.json    # one file per lane, diff-friendly
    bass.cutoff.json
  assets/              # audio, content-addressed
  layout.json          # node positions and UI state only
  tests/
    mix.test.ts
```

What's built today:
- `music2.toml` has only `[project]` with `name` (and `entry`, the root module, `song.ts` by default).
- `timeline/song.json` has only `tempo` (a list of `{beat, bpm}`) and `beats_per_bar`.
- Lane files are named `<track>.<lane>.json`. `<track>` is the track's name in lower case, with only the letters A to Z and the digits kept: every run of other characters (spaces, punctuation, accented letters) becomes one `-`, with none at either end. A track called `Lead Vox` has `lead-vox.notes.json`.
- `assets/` holds named files (WAVs, and plugin state in `assets/plugin-state/`).
- **Not built yet:** `music2.lock`, `layout.json`, `tests/`, sections, and content-addressed assets.

## Authoring

This is the shape of the real `examples/first-song` project, which builds and renders today. The numbers are illustrative: the example's own values may differ.

```ts
// tracks/kick.ts
import { ms, track } from "@m2/core";
import { follow } from "std/dsp";
import { kick } from "std/instruments";

export default track("Kick", ({ lane }) => {
  const out = kick(lane.notes("hits"));
  // Exported so other tracks can duck to the kick.
  return { out, env: follow(out, { release: ms(150) }) };
});
```

```ts
// tracks/bass.ts
import { hz, khz, track } from "@m2/core";
import { blend, lfo } from "std/control";
import { bass } from "std/instruments";
import { xrange } from "std/math";

export default track("Bass", ({ lane, time }) => {
  const cutoff = lane.control("cutoff", "hz", hz(500)); // automation, drawn on the timeline
  const drop = lane.mask("drop"); // 1 where the wobble should take over

  const wobble = xrange(lfo(time.beats, { period: 0.5 }), hz(200), khz(2.5));
  return bass(lane.notes("notes"), { cutoff: blend(cutoff, wobble, drop) });
});
```

```ts
// song.ts
import { song, db } from "@m2/core";
import { duck } from "std/fx";
import { master, mixer } from "std/mix";
import bass from "./tracks/bass";
import hats from "./tracks/hats";
import keys from "./tracks/keys";
import kick from "./tracks/kick";

export default song(() =>
  master(
    mixer({
      kick: kick.out,
      hats: { in: hats.out, db: db(-6), pan: 0.3 },
      // Sidechain: the bass ducks whenever the kick hits.
      bass: duck(bass.out, kick.env, { depth: db(-9) }),
      keys: { in: keys.out, db: db(-5), pan: -0.25 },
    }),
  ),
);
```

`bass()` from `std/instruments` is an ordinary function that builds a `poly` voice out of `std/dsp` parts. It is wrapped in `component()`, which tells the editor that `cutoff` and `drive` are knobs, so a `bass(...)` call shows as one box:

```ts
export const bass = component({ controls: ["1.cutoff", "1.drive"] }, function bass(notes: Events, o: { cutoff?: Value; drive?: Value; voices?: number } = {}): Audio {
  const cutoff = o.cutoff ?? hz(600);
  const v = poly(
    notes,
    (n) => {
      const body = add(osc.saw(n.pitch), scale(osc.square(add(n.pitch, -12)), 0.5));
      const env = adsr(n.gate, { attack: ms(3), decay: secs(0.3), sustain: 0.6, release: ms(80) });
      const fenv = adsr(n.gate, { attack: ms(2), decay: secs(0.18), sustain: 0.2, release: ms(80) });
      const f = svf.lowpass(body, xrange(fenv, cutoff, scale(cutoff, 4)), 1.1);
      return vca(f, scale(env, n.velocity));
    },
    { voices: o.voices ?? 4 },
  );
  return drive(v, o.drive ?? 4);
});
```

There is no tempo and there are no sections in this code. Both are timeline data: the tempo is in `timeline/song.json`, and sections are **Not built yet**.

### Editing code from the graph

**The TypeScript is the source of truth. The node editor is a live view of it that can also edit it,** like SwiftUI code and Xcode's canvas. The problem was handed to a specialist ([brief](code-graph-editing-brief.md)), whose proposal is in [`code-graph-editing-design.md`](code-graph-editing-design.md).

**Decision (2026-10-11).**

Adopted from the proposal:
- **Identity belongs to a call occurrence (a *site*), not a binding name.** Unnamed inline calls are as editable as named ones, and nothing forces every value into a binding. This replaces my earlier rules 1 and 2.
- **Controls vs settings.** Controls are numbers that can change while playing; settings shape the graph (voice counts, modes). A function declares which arguments are controls with `component({ controls: ["1", "0.*.pan"] }, fn)`. Each plain number at a control path becomes an addressable constant.
- **Three states:** draft (files on disk), accepted (the last revision that compiled) and preview (a live value on the accepted plan).
- **Verification in a candidate snapshot built from memory.** Files are written only after an edit verifies, and never written then rolled back.
- **Small edits at byte ranges.** Files are never reprinted.
- **Constants stay addressable through optimisation,** so a preview can reach them.
- **Exact call sites by instrumentation.** I tested the alternative: QuickJS's stack positions can't tell nested calls apart (`svf.lowpass(...)` and the `vca(...)` around it report the same column).

Simplified for now:
- **Verification:** "the candidate's graph equals the accepted graph with exactly this change". Components aren't re-expanded in isolation under an effect contract.
- **Concurrency:** re-read from disk before every edit, an `expect` value guard, and a write guard. There is no semantic read-set tracking yet.
- **History:** in memory. The durable journal, crash recovery, editor bridge and preview leases come later.

**Built (first slice):**
- **Call-site instrumentation** (`core/script/src/sites.rs`). Each call records its site right before it is invoked: `f(a, __m2s(7, b))`, `__m2s(7, obj).m()`, `(__m2s(7), f)()`. Evaluation order and `this` are unchanged. A site table keeps every call's byte range and argument structure.
- **Exact source locations.** The IR's `origin` names the site and argument path of every node, so `src` locations are exact.
- **The session** (`core/session/src/edit.rs`):
  - `controls()` lists every number in the project's own files that is written directly at a control input; those are the knobs. A control's id is `file#call/arg`, e.g. `song.ts#2/0.hats.pan`.
  - `set(id, value, expect)` writes the shortest decimal that reads back exactly, keeping unit spelling (`khz(2.5)` set to 900 Hz becomes `khz(0.9)`). It verifies, writes, and bumps the revision.
  - `undo()` is guarded.
  - `refresh()` keeps the accepted revision playing while a draft doesn't compile.
- **Live preview:** `Renderer::set_const(origin, value)` changes a constant everywhere it is used, voices included, at the next block. A test checks that the preview renders bit-identically to the committed edit.
- **CLI:** `m2 controls <project>` and `m2 set <project> <id> <value> [--expect V]`. A verified edit of the example takes about 5 ms after opening.

**Correctness rules** (after review, each with a regression test):
- **A revision is immutable and complete.** It is one snapshot of every text file (code, timeline, manifest) read once, the project parsed from it, and the graph compiled from it. Everything shown or played comes from the accepted revision. A draft that doesn't compile, including bad timeline data, never replaces any part of it.
- **Edits name the revision they were planned against.** If the files changed since, the target is carried forward only if no change touched it: its call lies wholly outside the changed bytes and reads the same in place. Otherwise the edit is refused as stale. A carried-forward edit never lands on a different knob.
- **Instrumentation only where it provably keeps meaning:** calls with arguments, plain `f()`, and `obj.m()` with no optional link. `(obj.m)()`, `obj.m!()`, `super.m()` and optional chains are left alone; they get no editable site but keep their exact meaning. An end-to-end test runs these forms through the instrumented build.
- **Undo restores the original bytes** (`0x6e`, `1_10`, `(110)` …) and verifies with the value it recorded, rather than reparsing JavaScript number syntax.

#### The canvas today

In the desktop app, *File → Open Project Folder…* (⌘O), or `music2-desktop <project-folder>`, opens a project. The window shows the timeline above a panel with a tab for each module, and three views of it: Graph, Code and Notes. The README describes how to use them.

- **The view** (`core/session/src/view.rs`) shows the calls written in the project, not the kernels they expand into. `component()` records which call each node was made inside, so everything `bass(...)` builds is one node.
  - Knobs sit on the innermost call that contains where their number is written.
  - Knobs are named by property path (`hats.pan`, `hats.db`) or by the port they feed (`hi`, `period`), with a range from their unit (log for Hz, seconds and beats).
  - `m2 graph <project>` prints the same view.
- **Knobs and playback** (`core/play`, `m2-play`, the core's one touch point with the audio device). A knob drag previews live (`set_const`, sent at most every 30 ms). Releasing commits through the session.
  - The running renderer isn't rebuilt for a value commit, because the committed graph equals the playing one with that value.
  - Edits made elsewhere are picked up every 300 ms and rebuild the renderer at the current position.
  - `M2_AUDIO=none` runs it silently on a clock, for tests.
- **Cables** (`core/session/src/edit/cable.rs`), within one file. An outlet is a `const` bound to a call that returned one signal. Connecting writes its name in place of a control's written value; disconnecting writes the default the input declares, and is refused when it declares none.
- **Effects** (`core/session/src/edit/effect.rs`). The effect browser lists the `[[effect]]` entries of the loaded packages: 17 in std, in seven categories. Adding one wraps a call's audio where it is written (`pluck(…)` → `reverb(pluck(…), {…})`); removing one unwraps it.
- **The timeline** (`web/src/components/SongTimeline.tsx`) shows every lane of every track. Notes clips can be added, moved and deleted there; automation, mask and audio clips are shown only.
- **Notes editing** (`NoteView.tsx`, `core/session/src/lanes.rs`): the piano roll rewrites one notes lane's clips at a time, checked like any other edit.
- **The code editor** (`CodeView.tsx`, `crates/server/src/files.rs`): Monaco, type-checked against the SDK and the package sources. A save is written only if the file still says what the editor read. Code that doesn't compile is saved too, and the last revision that did keeps playing.
- **Undo and redo** cover every edit made through the session (knobs, cables, effects, notes, code saves). The history is in memory.
- **Server** (`crates/server/src/routes.rs`):
  - `GET /api/graph`, `/api/graph/status`, `/api/graph/files` and `/api/graph/types`.
  - `POST` under `/api/graph/`: `open`, `close`, `preview`, `preview/cancel`, `set`, `connect`, `disconnect`, `effect/add`, `effect/remove`, `lane`, `undo`, `redo`, `transport`, `seek`, `unsaved` and `export`.
  - `PUT /api/graph/file`.
  - New songs: `GET /api/project/templates` lists the templates (`templates/`) and the examples; `POST /api/project/new` copies one into a new folder in `~/Music/music2` (or a folder given) and opens it (`crates/server/src/project.rs`).
  - Export Audio (`crates/server/src/export.rs`) renders the accepted revision on a thread of its own with `m2_session::render`, the same render `m2 render` uses.
  - Clients refresh on the `{"t":"graph"}` event from the `/api/ws` WebSocket, which also carries `{"t":"toast"}` messages and `{"t":"export"}` progress.
- **A headless UI test** (`crates/server/examples/headless.rs` plus a puppeteer script) caught a real bug: releasing a knob committed a stale mid-drag value (0.65) while you heard the final one (0.7). It now commits exactly the value shown at release, and a fast-drag check guards it. The puppeteer script isn't in the repo (puppeteer isn't in `web/package.json`); the headless server is.

**Next:** cables across files (exports and imports), adding and deleting nodes, automating a knob, and saved node positions.

## Agents

Agents use the files, `~/.music2/session.json` and the `/api/graph` routes; MCP is not built yet. In the design, agents connect to the host's MCP server, whose tools are generated from the core API and installed tools.

### What agents write

| Kind of edit | How they make it | Default stance |
|---|---|---|
| **Graphs**: sound design, chains, routing | edit TypeScript, with the compiler in the loop | **The biggest win.** Typed code plus structured diagnostics is the loop language models are best at. |
| **Controls**: automation, masks | `points.put` on lanes | Strong. "Open the filter over the build", "glitch only in the chorus". |
| **Notes** | `events.put`, or a `std/gen` generator that the artist can `bake` into data | **Propose, don't overwrite.** This is where the artist's taste lives. |
| **Packages** | write new modules or kernels, with tests and an example | Agents can grow the ecosystem themselves. |

**Rule of thumb:** anything the artist will hand-tweak is data; anything systematic is code. `bake` turns code into data whenever the artist wants to take over.

### Ears

**Not built yet.** Agents can't listen, so they need another way to judge the result. `render` with `std/meters` taps, plus the host's analysis tool, gives them:
- loudness and peak per section
- spectral balance
- **masking between tracks** (e.g. "the bass is masking the kick at 60 Hz")
- key and onset detection
- stereo width
- **spectrogram images**, which multimodal models can look at directly

### Variants and locks

**Not built yet.**

- An agent works on a **variant**: a git branch or worktree of the project, managed by the host.
- The artist compares A and B in the app, then keeps, merges or throws away the variant.
- Locks (enforced by the core) protect whatever the artist marked as theirs.

### Example: "make the chorus vocal glitchy"

**Not built yet:** this uses the core API, packages, sections and taps from the design.

1. `project.get` (describe) shows the `vox` track, its exports and the `chorus` section.
2. `pkg.search "stutter glitch"` finds `github:someone/stutter@1.2` and shows its typed ports.
3. The agent opens variant `glitch-chorus` and sends one `tx`:
   - `pkg.add`
   - a `file.patch` on `tracks/vox.ts`:
     ```ts
     out: blend(dry, stutter(dry, { beats: time.beats, rate: "1/16" }), section("chorus"))
     ```
   The transaction compiles, or comes back with exact diagnostics.
4. It renders the chorus with taps, and compares loudness and spectrograms before and after.
5. It reports: "Variant `glitch-chorus` is ready to compare." The artist decides.

---

## Software engineering for free

- **Git:** code, timeline data and the lockfile all diff and merge as text.
- **Tests** (**Not built yet**) run inside the sandbox:
  ```ts
  test("drop doesn't clip", () => expect(render(song, section("drop"))).toPeakBelow(db(-1)));
  test("intro is quiet",   () => expect(render(song, section("intro"))).toLoudnessBelow(lufs(-18)));
  ```
- **Lint rules** (**Not built yet**) are package tools that read the IR and the timeline: notes outside the key, a bass masking the kick with no ducking, unused exports, a mask lane that is never used.
- **Incremental renders** (**Not built yet**): results are cached by hash, so an edit only re-renders what is downstream of it.

## Risks and open questions

1. **WebAssembly kernel speed.** Measured at 1.78× native (see Implementation status), which passed the M1 gate. Large sessions still need the parallel runtime model. If it's too slow, trusted packages could ship native code behind the same ABI; that would be the only exception to "no privileged packages".
2. **Speed of per-voice instances in `poly`.** Cloning a kernel's memory for each voice is fast in wasmtime. Running a voice body for many voices at once in a single kernel call can be added later without changing the ABI.
3. **Editing code from the graph** is the hardest engineering in the project and decides whether artists accept it. It is being designed separately (see the brief) and will be proven early on a real track file.
4. **Live input and recording.** Recording writes data to lanes. Monitoring live input needs a low-latency path that skips the cache, which is a core runtime concern.
5. **The JavaScript engine.** QuickJS is small and deterministic, but slow for very large generated graphs. Builds happen at edit time, not on the audio thread, so this should be fine. Revisit if needed.
6. **Kernel parameter changes while playing**, e.g. a sampler swapping its asset, require re-initialising the kernel. The planner treats this as replacing the node.
7. **Naming and governance of std versus the community:** who maintains `std`, and what is required to get into it.
8. **Plugin hosting** brings threading, crashes, latency reporting and state formats that differ per plugin. CLAP first keeps the first version small; running plugins out of process is the safety valve.
9. **Time-stretch quality** decides whether warped audio sounds professional. The std stretch kernel must be good, and the stretch kernel is a package so better ones can replace it.

## Implementation status

**M1, the core loop, is done.** `m2 render examples/first-song out.wav` runs the whole pipeline: TypeScript in the sandbox → IR → checker → WebAssembly kernels → WAV.

| Piece | Where |
|---|---|
| IR (eight ops), kinds and units, kernel signatures, the checker | `core/ir` (`m2-ir`) |
| Project folder, timeline files, tempo map | `core/song` (`m2-song`) |
| Package loading, kernel host, `poly`, `feedback`, audio clips, renderer | `core/runtime` (`m2-runtime`) |
| TypeScript sandbox (oxc + QuickJS) and the `@m2/core` SDK | `core/script` (`m2-script`, `sdk/core.ts`) |
| Kernel authoring kit for Rust | `core/kernel` (`m2-kernel`) |
| Compiling, revisions, the graph view, edits (values, cables, effects, notes lanes, file saves) and undo | `core/session` (`m2-session`) |
| CLAP plugin hosting | `core/plugins` (`m2-plugins`) |
| Playback on the audio device | `core/play` (`m2-play`) |
| The `std` package: manifest, 29 kernels, 17 effects, TypeScript modules | `std/` (kernels built with `std/build.sh`) |
| CLI: 16 commands (`new`, `check`, `render`, `kernels`, `bounce`, `graph`, `controls`, `set`, `cables`, `connect`, `disconnect`, `effects`, `add-effect`, `remove-effect`, `plugins`, `plugin-state`) | `apps/m2` |
| The app's server: the open project and playback (`graph.rs`), the code editor's files (`files.rs`), new songs (`project.rs`), Export Audio (`export.rs`), the bundled resources (`resources.rs`), the `/api/graph` and `/api/project` routes | `crates/server` (`m2-server`) |
| The desktop app and its UI: start screen, New Song dialog, timeline, graph, piano roll, code editor, effect browser, Export | `apps/music2-desktop`, `web/` |
| Song templates (Starter beat, Empty) | `templates/` |
| Example songs | `examples/first-song`, `examples/with-plugins`, `examples/resample` |

**Measured:**
- **The decision gate passes.** The same kernel source runs **1.62× slower as WebAssembly than native**: 16 voices of saw → filter → envelope render **48× faster than real time** (`cargo run --release -p m2-runtime --example bench`). The correctness milestone below (fixed schedule, exact note starts, strict checks) costs about 10%: now **1.78×**, 44× real time.
- The example song (4 tracks, 22 voices, 264 kernel instances) renders 17.5 s of audio in 0.51 s, and two renders are byte-identical.

**Correctness milestone (after review), done.** The render specification at the top of `core/runtime/src/lib.rs` is now what the code does, with a regression test for each rule:
- **A fixed block grid.** Audio is processed in 128-sample blocks from sample 0, whatever buffer size the caller asks for. The output is identical for buffer sizes 1, 7, 64, 100, 129 and 1000.
- **Feedback** is a delay of exactly 128 samples. It is no longer "the previous call".
- **Voice changes at the exact sample.** A voice changes note at the sample where the note starts. A stolen voice keeps playing its old note until then, and its graph is split at that point.
- **Voices end** when their note is over, their output is quiet, and no kernel in them is `active`, so echoes inside a voice survive note-off.
- **Seeking.** `seek(s)` resets and replays, so a section render is byte-identical to the same slice of a full render (`m2 render --from`).
- **NaN and infinity.** A non-finite sample is an error in strict mode (the default) that names the kernel, e.g. `song/hz produced inf at 0.000s`. In live mode it is silenced and counted.
- **`check` means playable.** `m2 check` and `m2 render` share one compile step (`m2-session`). It validates every lane's timeline data against the lane's kind and rejects track names that would share timeline files. It also rejects nonzero kernel latency and events feeding kernels until they are supported. (Audio lanes were rejected at first too; they work now, see below.) Unused lane files are warnings.
- **Packages** must not import anything, must export the full ABI, and must report the runtime's ABI version.

**Settled while building:**
- `std` is **one package** whose modules are imported as `std/dsp`, `std/math`, `std/control`, `std/filter`, `std/fx`, `std/mix` and `std/instruments`. Kernel ids are `package:kernel`, e.g. `std:svf`.
- Units in TypeScript are functions (`hz(800)`), and plain numbers are untyped literals.
- `feedback()` declares its kind up front (`feedback("audio<1>")`), and its input is given later with `.set()`.
- Inside `poly`, a voice can read any signal from its module. Lanes, imports and `time` are always created at module level, so voices never contain them.
- The sandbox deletes `Date`, makes `Math.random` throw, can only import project files, `@m2/core` and installed packages, and stops a build after 10 s or 256 MB.

**Experiment: hosted plugins (CLAP), done.** `core/plugins` (`m2-plugins`) hosts CLAP plugins, tested with Surge XT, Surge XT Effects and Airwindows Consolidated:
- A plugin is a graph node: `plugin("clap:<id>", { in, notes, <param>: value, state: "name" })`.
  - Notes reach it at their exact sample offsets, with tuning for pitches between keys.
  - Parameters are typed ports that can be automated from lanes, sampled every 16 samples.
  - Song position and tempo are passed in as transport.
- **An unconnected parameter keeps the plugin's own value** (a new input form, `Unset`). Without it, connecting nothing would have reset every knob of a loaded patch.
- **State is an asset:** `m2 plugin-state` saves it, and `check` fails if a referenced state is missing. Save and load round-trip exactly.
- `m2 plugins` lists installed plugins; `m2 plugins <id>` shows each parameter's port, range and default.
- **Plugins can't be inside `poly`:** an instrument plugin plays all the notes itself.

What it taught:
- **Purity is per plugin.** Surge XT renders differently every run (random oscillator phases), while Airwindows is bit-identical. The host could detect this by rendering twice and comparing, and bouncing is the answer for impure ones.
- **Much of a plugin's sound lives in its state,** not its parameters. Surge's default patch has the filter off; Airwindows chooses its algorithm in state. Code should be able to name stepped values (`a_filter_1_type: "LP 24 dB"`); `value_text` exists in the host for this.
- **Parameters are normalised 0..1,** so the `scalar` unit is right but says little. Display text per parameter is the useful information.

Not done yet: running plugins out of process, plugin editor windows, and latency compensation (plugins that report latency are rejected for now).

**Experiment: audio first, done.**
- **Audio lanes:** `lane.audio("clips")`. A clip carries an asset, offset, gain, fades, reverse and pitch, plus warp markers `[source_seconds, beat]`.
- **Pitch** without warp markers is varispeed; with them, timing follows the markers and only pitch changes.
- **Rendering:** clips are rendered once when the renderer is built, so playback is a copy, and seeking and block-size invariance hold exactly.
- **Warping** uses WSOLA with any time map. A sine stretched to half or double speed keeps its frequency exactly, with no level ripple. Assets at other sample rates are resampled (cubic).
- **Bounce:** `m2 bounce <project> <name> --track T [--place Track.lane@beat]` renders any export to a 32-bit float asset, with a provenance file that records whether it's reproducible (no plugins).
  - A bounce placed back as a clip plays bit for bit like the original.
  - `examples/resample` runs the whole loop: bounce a synth, then chop, reverse, pitch and stretch it.

What it taught:
- **Pre-rendering clips is simple and exact,** but costs memory for long sessions. The realtime engine will stream from a disk cache keyed by the clip's content.
- **Stretching is in the core runtime for now** (`core/runtime/src/audio.rs`). Moving it into a package, as the design wants, needs an *offline kernel* ABI: whole buffers in, whole buffers out, with a time map. That's a small, well-defined extension.

**Not built yet** (each belongs to a later milestone):
- latency compensation (rejected at check time until then)
- sections
- stable ids from source positions (ids are counters until hot swap needs them)
- per-kernel CPU budgets
- event-producing kernels: today only a notes lane can feed `poly`
- render caching and checkpoints (seeking replays from 0)

## Build plan

The old engine was removed in October 2026; the app runs only on this core. After review, the plan proves editing early and defers breadth. For the list of what's missing from a user's point of view, most important first, see [What music2 can't do yet](gaps.md).

1. **The core loop, end to end (decision gate).** Done: see [Implementation status](#implementation-status). Imports, cycle diagnostics, `feedback`, `std/mix` and `follow`/`duck`, originally planned for later, shipped here too.
2. **Correctness.** Done: a fixed block grid, exact note starts, voice tails via `k_active`, seek by replay, strict NaN errors, and `check` meaning playable.
3. **Three experiments, before building the session,** because each shapes the session and runtime. Partly done:
   - **Code ↔ graph editing:** open `tracks/bass.ts` as a graph; drag a knob, rewire a cable, add and delete a node, automate a parameter; every change lands as a minimal, compiler-verified code edit (see the brief). Partly done: knobs, cables within one file, and adding and removing effects. Adding and deleting other nodes, and automating a parameter, are not built yet.
   - **A hosted plugin:** done (see Implementation status).
   - **Audio first:** done (see Implementation status); the stretch kernel still needs to move into a package.
4. **The editing session** (`m2-session`). Partly done:
   - stable ids for tracks, lanes, clips and notes: not built yet
   - project revisions: done
   - atomic transactions over code and timeline data, each carrying its base revision so concurrent edits can't silently overwrite each other: partly done (each edit carries its base revision; there are no multi-edit transactions)
   - undo: done, in memory
   - **draft versus accepted:** files on disk are the draft, and the engine plays the last revision that validated: done
   - locks, checked at that acceptance point: not built yet
   - local packages pinned by hash before any fetching from git: not built yet
5. **One complete workflow in the desktop app:** open the example → play → move a clip → edit automation → change code while a note is held → have an invalid edit rejected → undo → save and reopen. Seeking uses reset-and-replay at first. This milestone also includes hot swap with stable ids, real-time playback, and the parallel, realtime-safe runtime model. Partly done: everything but editing automation works, with seeking by replay and real-time playback. Hot swap with stable ids and the parallel runtime are not built yet.
6. **The node editor on the code-first model:** the house-style operations from the experiment, in the app. Partly done: knobs, cables and effects.
7. **The agent surface:** MCP generated from the session API, `render` taps, `std/meters`, the analysis tool and spectrograms, variants. Not built yet.
8. **Breadth:** a complete `std` (port the missing instruments and effects: lead, keys, strings, drums, sampler, pump), sections, latency compensation, audio lanes, git package resolution, render caching and checkpoints. Partly done: audio lanes work, and std has grown to 17 effects; the rest is not built yet.
9. **The full node editor:** group nodes, opening project modules, multi-file edits. Not built yet.
