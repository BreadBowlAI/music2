# Song as a program

The design for the next music2. Music production as a software engineering problem.

Status: final draft (2026-10-11). Milestone 1 (the core loop) is implemented; see [Implementation status](#implementation-status).

## Summary

**A song is a program whose only input is time.**

- **What a song is made of:**
  - **Graphs** describe how it sounds. They are written in TypeScript or edited as nodes.
  - **Timeline data** is what gets played: notes, automation, masks, audio clips and tempo.
  - The graph is the source of truth for behaviour. Per-clip behaviour comes from masks, never from per-clip changes to the graph.
- **How it runs:** a compiler checks the whole song, and a small Rust core plays it.
- **The core contains no instruments, effects or DSP.** Oscillators, filters, synths, mixers, LFOs and arrangement helpers all live in a **standard library** that ships the same way as any community package. The core only:
  - checks types
  - schedules and runs graphs
  - hosts sandboxed DSP code
  - stores the project
- **Who uses it:** artists build songs from packages that others publish. AI agents work on the same project through the same API as the GUI, but they never get the final say.

## Principles

1. **The minimal-core rule.** Something belongs in the core only if it can't be a package, or if making it a package would break a guarantee: safety, determinism, or a single source of truth. Everything else is a package.
2. **No privileged packages.** The standard library uses the same plugin interface as everyone else. If the standard library can't be built as an ordinary package, the plugin system is missing something, and that gets fixed in the core.
3. **Time is the only invariant.** Every signal is a function of song time. Kernels (compiled DSP code) see time only through their input ports.
4. **Deterministic.** The same project and lockfile always give the same audio, bit for bit, on every machine. That makes caching, testing and reproducibility work.
5. **Files are the truth.** A project is a folder of text and assets, and it diffs in git. The engine is the only writer at runtime, and every client goes through it.
6. **Behaviour, data and layout are kept apart:**
   - Code is behaviour.
   - The timeline is data.
   - `layout.json` only controls how things are drawn.
7. **Agents are clients, not features.** There is no AI built into the app. Agents get the same commands as the GUI, plus a way to "hear" the song, plus variants they can propose.
8. **The artist decides.** Agents propose on variants, and locks protect what the artist doesn't want touched.

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
- Derived clocks are ordinary standard-library functions of `time.beats`. Examples: `every("1/8")`, `swing(0.6)`, `div(3)`, `phase(...)`. Polyrhythm and swing need nothing special in the core.

### Signals

There are three kinds of signal, and every connection has exactly one kind:

| Kind | Meaning | Rate |
|---|---|---|
| `Audio<Ch>` | sound, mono or stereo | sample rate |
| `Control<U>` | a continuous value with a unit | block rate, smoothly interpolated within each block |
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
  velocity: number;             // 0..1
  gate: Control<Unit01>;        // 1 while held
  pressure: Control<Unit01>;
  timbre: Control<Unit01>;
};
```

### Nodes

A node is one of two things:
- a **kernel instance**: compiled DSP from a package
- a **module instance**: a graph from a package or from your project

Every node is:
- **Causal.** Its output at time `t` depends only on its inputs up to `t`, plus any **look-ahead** it declares.
- **Deterministic.** Kernels run in a sandbox with no access to a clock, randomness, threads or I/O. Random numbers come from seeds passed in as inputs.
- **Explicit about latency.** Every kernel declares its latency, and the planner compensates automatically.

### Lanes and clips

A **lane** is a typed input of a module that the timeline fills in.

| Lane | Signal | What the clips contain |
|---|---|---|
| `lane.notes(id)` | `Events<Note>` | notes |
| `lane.control(id, unit, default)` | `Control<U>` | breakpoints with curves |
| `lane.mask(id)` | `Control<Unit01>` | breakpoints, default 0 |
| `lane.audio(id)` | `Audio` | references to assets, with gain and fades |

The song itself also has lanes for **tempo** and **sections**. Sections are arrangement markers such as `intro` and `chorus`, stored as `Events<Section>`.

**Clips are always data and never code.** Where clips overlap on a lane, the later one wins. In gaps, the lane's default value applies.

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
poly(notes, n => voice(n), { voices: 16, steal: "oldest" })
```

`poly` creates one instance of the voice graph for each sounding note and sums the outputs. A voice ends once its note has ended and either its output has dropped below silence or a declared tail time has passed. `poly` is in the core because it changes the shape of the graph while it runs, which no package can do.

### References between modules

**The graph only pulls.** A consumer imports a producer's export: `duck(bass.out, kick.env)`. Nothing pushes.

- The planner sorts the whole flattened song into a processing order.
- **A cycle is a compile error**, and the error names the loop.
- Intended feedback must go through `feedback(signal, { delay })`, which delays by at least one processing block.
- A "send" in the GUI is a view of an edge that the bus pulls in.

---

## The core

This is the whole core. If something isn't listed here, it lives in a package or a host.

### 1. Graph IR: seven ops

The IR is the compiled form of a song: a JSON graph of nodes. Every graph, however it was authored, compiles down to these seven ops:

| Op | Meaning |
|---|---|
| `lane` | a module's typed timeline input |
| `time` | `beats` / `seconds` |
| `const` | a typed constant (a number with a unit, or an asset reference) |
| `kernel` | an instance of a package kernel, with its port connections |
| `poly` | maps a voice subgraph over `Events<Note>` and sums the results |
| `feedback` | an explicitly delayed edge, the only way to form a loop |
| `import` | another module's export |

Each module also lists its `exports`. The **validator** checks the IR independently of whatever produced it:
- signal kinds and units
- ports exist and are connected
- no cycles
- rates, latency and look-ahead are consistent

### 2. Planner

The planner turns validated IR into a plan the runtime can execute:
- flatten modules into one graph
- sort into a processing order
- insert latency compensation
- assign buffers
- prune anything that doesn't reach the root output
- assign each node a **stable id** derived from its position in the source (overridable with `.id()`)

### 3. Runtime and kernel host

- Processes audio in blocks, either in real time to the audio device or offline as fast as possible.
- **Hot swap:** a new plan is compiled on a worker thread and handed to the audio thread without locking. Nodes whose stable ids match carry their state over, so reverb tails don't cut off and held notes keep sounding.
- **The kernel ABI: WebAssembly**, run with wasmtime (Cranelift). Relaxed SIMD is disabled, so output is bit-identical on every machine.
  - NaN bit patterns are the one thing WebAssembly doesn't pin down. Canonicalising them cost 4.6× in the M1 benchmark, so the runtime doesn't. Instead it silences any non-finite output sample and reports it, because a NaN means a kernel is broken anyway.
  - A package ships one `.wasm` module that can hold many kernels, and every graph node gets **its own instance**, so a kernel's state is just its instance's memory. For a kernel named `k`, the module exports:
    - `k_init(sample_rate, max_block, channels)`
    - `k_ports()`: the number of inputs and outputs, which the host checks against the manifest
    - `k_port(i)`: the address of port buffer `i` (inputs first, then outputs; channel-major, `2 × max_block` floats)
    - `k_reset()`
    - `k_process(frames)`
    - plus `m2_abi()` and `memory`, once per module
  - **No host imports.** A module that imports anything is rejected at load, so a kernel can't reach anything outside its own ports.
  - Each instance's memory is capped (32 MB). A CPU budget is still to come.
  - Rust kernels use the `m2-kernel` crate: implement `Kernel` and call `export!(name, Type)`.
  - Assets (for samplers and impulse responses) are passed in as read-only data when the kernel starts.
- **Render and taps.** `render(range, taps)` renders a time range and returns audio, plus the values of any signal you ask for (for example, a meter kernel's output). Rendered results are **cached by a hash of what produced them** and invalidated only downstream of an edit. Freezing a track just means keeping its cache.
- **The audio device:** output, input and MIDI in. This is the one place the core touches hardware.

### 4. Script sandbox

- Strips TypeScript types (oxc) and runs the builder in an embedded JavaScript engine (QuickJS).
- The sandbox has no file access, no network and no clock, and randomness only comes from seeds. Running a module can only **describe a graph**.
- That sandbox is what keeps code packages as safe to install as data, which today's `m2plug` format guarantees by being pure data.
- The `@m2/core` TypeScript SDK is part of the core. It is small: the signal and unit types, `track`/`module`/`song`, `lane`, `poly`, `feedback`, and graph-handle recording.
- Typed bindings for kernels will be **generated from package manifests**. In M1, std's wrappers (`std/dsp` and the rest) are written by hand over `kernel("std:svf", {...})`. Running `tsc` is an optional extra check for editors; it is never what decides whether a song compiles.

### 5. Project, transactions and undo

- Reads and writes the project folder and watches it for edits made from outside.
- Every change is a **transaction**:
  1. Apply the ops.
  2. Recompile.
  3. Validate.
  4. Commit, or reject with nothing changed.
- When a transaction fails, it returns **structured diagnostics** that point at both the source location and the graph node, so an agent can fix its own mistake.
- **Undo** works per transaction, whether the edits were to code, data or both.
- **Locks** (in the manifest) are enforced here, because the core is the only writer:

  ```toml
  [locks]
  "tracks/vox.ts" = "graph"      # graph frozen, lanes editable
  "timeline/vox.*" = "all"
  ```

### 6. Package resolution

- Resolves `music2.toml` dependencies, fetches packages (git sources such as `github:owner/repo@tag`, or bundled ones), verifies their hashes, and writes `music2.lock`.
- Checks that kernel manifests are compatible with the ABI version.
- The standard library is resolved exactly like any other package. The app ships a copy so it works offline, but the song's lockfile pins its version independently of the app.

### 7. The core API

These are the only commands the core provides. Hosts expose them over HTTP/WS and MCP, and the CLI calls them directly.

| Command | Purpose |
|---|---|
| `project.get` | files, compiled IR, diagnostics and revision, or a compact `describe` summary |
| `tx` | an atomic batch of ops, compiled once and undone as one step |
| `undo` / `redo` | |
| `check` | compile without committing; returns diagnostics |
| `render` | a time range and taps, producing a WAV and tapped signals |
| `transport` | play, stop, seek, loop, live input monitoring |
| `subscribe` | patches whenever the revision changes (as the current WebSocket sync does) |

The ops a `tx` can contain:
- **Files:** `file.write`, `file.patch` (text edits)
- **Timeline:** `clip.put`, `clip.delete`, `events.put`, `events.delete`, `points.put`, `points.delete`
- **Packages:** `pkg.add`, `pkg.remove`

Higher-level edits are **tools**: pure TypeScript functions from packages that take data and return `tx` ops. Examples include quantize, transpose, humanize, the node editor's code edits, and generators. The core's command set never grows.

### Not in the core

These live elsewhere on purpose:
- Every oscillator, filter, envelope, LFO, effect, instrument and the mixer (packages)
- Derived clocks, sections-as-masks, mute and solo (packages)
- Meters and analysis (meters are package kernels; spectrograms and reports come from a host tool)
- The GUI, node editor, piano roll, MCP server and variants (hosts)
- CLAP/VST/AU bridges (an optional host feature, and **not** reproducible)

---

## The standard library

`std` is a set of ordinary packages, versioned separately from the app. Its first versions are ported from today's `m2-dsp` and the built-in `m2plug` plugins.

| Package | Contents | Kind |
|---|---|---|
| `std/math` | `add`, `mul`, `scale`, `clamp`, unit conversions (`dbToGain`, `semiToHz`, …) | kernels |
| `std/dsp` | `osc` (saw/square/sine/tri, unison, FM), `noise`, `adsr`, `svf`, `biquad`, `vca`, `pan`, `delay`, `reverb`, `chorus`, `comp`, `drive`, `limit`, `follow`, `sampler` | kernels |
| `std/time` | `every`, `swing`, `div`, `phase`, `ramp` | kernels |
| `std/control` | `lfo`, `blend`, `when`, `slew`, `step`, `random(seed)` | kernels and modules |
| `std/arrange` | `section(name)` → gate, `mute`, `solo`, `fadeIn`, `fadeOut` | modules |
| `std/mix` | `mixer` (channel gain, pan, mute/solo lanes, sends), `bus`, `master` | modules |
| `std/instruments` | `pluck`, `lead`, `pad`, `bass`, `keys`, `strings`, `drums`, `sampler` (ported from the current plugins) | modules |
| `std/fx` | `space`, `echo`, `pump`, `shimmer`, `eq`, `glue`, `duck`, `filter`, `master` (ported from the current plugins) | modules |
| `std/meters` | `peak`, `rms`, `lufs`, `spectrum`, `onset`, `pitch` | kernels (used through `render` taps) |
| `std/gen` | `euclid`, `arp`, `chords`, `scale`, `strum` | tools (generators that output lane data) |
| `std/edit` | `quantize`, `transpose`, `humanize`, `legato`, `bake` | tools |
| `std/theory` | keys, scales, chord names | plain TypeScript, ported from `m2-theory` |

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

### Three levels of trust

| Level | What | Safety | Reproducible |
|---|---|---|---|
| Graph modules and tools | TypeScript run in the sandbox | describe graphs only | yes |
| Kernels | WebAssembly with no host imports | memory- and CPU-bounded | yes, bit-identical |
| Native bridges (CLAP/VST3/AU) | existing commercial plugins | full trust; opt-in, with a warning | no: marked impure, never cached |

### Versioning and lockfile

- **A package's ports and parameters are its API.** Changing their types is a breaking change and requires a major version bump.
- On upgrade, the compiler checks your song against the new signatures.
- `music2.lock` pins every package by **content hash**, so a song still renders identically years later.

### Making packages easy to find, for people and agents

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

## Authoring

This is the real `examples/first-song` project, which builds and renders today.

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

`bass()` from `std/instruments` is an ordinary function that builds a `poly` voice out of `std/dsp` parts:

```ts
export function bass(notes: Events, o: { cutoff?: Value; drive?: Value } = {}): Audio {
  const cutoff = o.cutoff ?? hz(600);
  const v = poly(notes, (n) => {
    const body = add(osc.saw(n.pitch), scale(osc.square(add(n.pitch, -12)), 0.5));
    const env = adsr(n.gate, { attack: ms(3), decay: secs(0.3), sustain: 0.6, release: ms(80) });
    const fenv = adsr(n.gate, { attack: ms(2), decay: secs(0.18), sustain: 0.2, release: ms(80) });
    const f = svf.lowpass(body, xrange(fenv, cutoff, scale(cutoff, 4)), 1.1);
    return vca(f, scale(env, n.velocity));
  }, { voices: 4 });
  return drive(v, o.drive ?? 4);
}
```

There is no tempo and there are no sections in this code. Both are timeline data, edited on the song's own lanes.

### Node editor and code, kept in sync

- **The TypeScript is the source of truth.** The editor draws nodes from the IR (each node points back to its source location), placed using `layout.json`.
- A GUI edit is a `std/edit`-style tool that outputs `file.patch` ops. Those make small changes to the code's syntax tree and keep formatting and comments.
- **Canonical subset:** straight-line `const x = node(...)` code can be fully edited in the GUI. Loops, conditionals and helper functions appear as collapsed **group nodes** with typed ports, which you can open to see the code. This is how node groups work in Blender.

---

## Agents

Agents connect to the host's MCP server, or use the files plus `~/.music2/session.json` as they do today. The MCP tools are generated from the core API and installed tools.

### What agents write

| Kind of edit | How they make it | Default stance |
|---|---|---|
| **Graphs**: sound design, chains, routing | edit TypeScript, with the compiler in the loop | **The biggest win.** Typed code plus structured diagnostics is the loop language models are best at. |
| **Controls**: automation, masks | `points.put` on lanes | Strong. "Open the filter over the build", "glitch only in the chorus". |
| **Notes** | `events.put`, or a `std/gen` generator that the artist can `bake` into data | **Propose, don't overwrite.** This is where the artist's taste lives. |
| **Packages** | write new modules or kernels, with tests and an example | Agents can grow the ecosystem themselves. |

**Rule of thumb:** anything the artist will hand-tweak is data; anything systematic is code. `bake` turns code into data whenever the artist wants to take over.

### Ears

Agents can't listen, so they need another way to judge the result. `render` with `std/meters` taps, plus the host's analysis tool, gives them:
- loudness and peak per section
- spectral balance
- **masking between tracks** (e.g. "the bass is masking the kick at 60 Hz")
- key and onset detection
- stereo width
- **spectrogram images**, which multimodal models can look at directly

### Variants and locks

- An agent works on a **variant**: a git branch or worktree of the project, managed by the host.
- The artist compares A and B in the app, then keeps, merges or throws away the variant.
- Locks (enforced by the core) protect whatever the artist marked as theirs.

### Example: "make the chorus vocal glitchy"

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
- **Tests** run inside the sandbox:
  ```ts
  test("drop doesn't clip", () => expect(render(song, section("drop"))).toPeakBelow(db(-1)));
  test("intro is quiet",   () => expect(render(song, section("intro"))).toLoudnessBelow(lufs(-18)));
  ```
- **Lint rules** are package tools that read the IR and the timeline: notes outside the key, a bass masking the kick with no ducking, unused exports, a mask lane that is never used.
- **Incremental renders:** results are cached by hash, so an edit only re-renders what is downstream of it.

## How it maps from today's music2

| Today | Next |
|---|---|
| `m2-time` | core (time and the tempo map) |
| `m2-dsp` | ported to `std/dsp` kernels, compiled to WebAssembly |
| `m2-graph` node types and `/plugins/*.m2plug.json` | `std/dsp`, `std/instruments`, `std/fx` |
| `m2-graph` `def.rs` and the validator | the starting point for the core IR and its validator |
| `m2-graph` `runtime.rs` and `m2-render` | the starting point for the core planner and runtime |
| `m2-engine` session, commands, `execute_steps`, undo | core transactions and undo, with a much smaller command set |
| `notes.quantize`, `notes.transpose`, … | `std/edit` tools |
| `m2-theory` | `std/theory` |
| `install_github_plugin` | core package resolution and the lockfile |
| `m2-project` (song struct) | project folder plus timeline data files |
| `server` (token, `session.json`, WebSocket patches) | a host: the same, plus MCP |
| desktop app and React UI | a host: timeline, piano roll and mixer views; the node editor is new |

## Risks and open questions

1. **WebAssembly kernel speed.** Ahead-of-time compiled WebAssembly should come within about 1.5× of native when processing blocks. M1 benchmarks it against the native `m2-dsp` code, and the result is a go/no-go decision. If it's too slow, trusted packages could ship native code behind the same ABI; that would be the only exception to "no privileged packages".
2. **Speed of per-voice instances in `poly`.** Cloning a kernel's memory for each voice is fast in wasmtime. Running a voice body for many voices at once in a single kernel call can be added later without changing the ABI.
3. **GUI edits beyond the canonical subset.** The group-node fallback has to feel good, not like a dead end.
4. **Live input and recording.** Recording writes data to lanes. Monitoring live input needs a low-latency path that skips the cache, which is a core runtime concern.
5. **The JavaScript engine.** QuickJS is small and deterministic, but slow for very large generated graphs. Builds happen at edit time, not on the audio thread, so this should be fine. Revisit if needed.
6. **Kernel parameter changes while playing**, e.g. a sampler swapping its asset, require re-initialising the kernel. The planner treats this as replacing the node.
7. **Naming and governance of std versus the community:** who maintains `std`, and what is required to get into it.

## Implementation status

**M1, the core loop, is done.** `m2 render examples/first-song out.wav` runs the whole pipeline: TypeScript in the sandbox → IR → checker → WebAssembly kernels → WAV.

| Piece | Where |
|---|---|
| IR (seven ops), kinds and units, kernel signatures, the checker | `core/ir` (`m2-ir`) |
| Project folder, timeline files, tempo map | `core/song` (`m2-song`) |
| Package loading, kernel host, `poly`, `feedback`, renderer | `core/runtime` (`m2-runtime`) |
| TypeScript sandbox (oxc + QuickJS) and the `@m2/core` SDK | `core/script` (`m2-script`, `sdk/core.ts`) |
| Kernel authoring kit for Rust | `core/kernel` (`m2-kernel`) |
| The `std` package: manifest, 20 kernels, TypeScript modules | `std/` (kernels built with `std/build.sh`) |
| CLI: `m2 build`, `m2 render`, `m2 kernels` | `apps/m2` |
| Example song | `examples/first-song` |

**Measured:**
- **The decision gate passes.** The same kernel source runs **1.62× slower as WebAssembly than native**: 16 voices of saw → filter → envelope render **48× faster than real time** (`cargo run --release -p m2-runtime --example bench`).
- The example song (4 tracks, 22 voices, 264 kernel instances) renders 17.5 s of audio in 0.46 s, and two renders are byte-identical.

**Settled while building:**
- `std` is **one package** whose modules are imported as `std/dsp`, `std/math`, `std/control`, `std/fx`, `std/mix` and `std/instruments`. Kernel ids are `package:kernel`, e.g. `std:svf`.
- Units in TypeScript are functions (`hz(800)`), and plain numbers are untyped literals.
- `feedback()` declares its kind up front (`feedback("audio<1>")`), and its input is given later with `.set()`.
- Inside `poly`, a voice can read any signal from its module. Lanes, imports and `time` are always created at module level, so voices never contain them.
- The sandbox deletes `Date`, makes `Math.random` throw, can only import project files, `@m2/core` and installed packages, and stops a build after 10 s or 256 MB.

**Not built yet** (each belongs to a later milestone):
- latency compensation (every std kernel reports 0)
- audio lanes
- sections
- stable ids from source positions (ids are counters until hot swap needs them)
- per-kernel CPU budgets
- event-producing kernels: today only a notes lane can feed `poly`

## Build plan

New code lives alongside the current crates in this repo. The old engine keeps running the app until M6.

1. **The core loop, end to end (decision gate).**
   - Core: IR with the seven ops, kinds and units, the validator, the planner, the runtime with the wasmtime kernel host, and the script sandbox.
   - First kernels in `std/dsp`: `osc`, `adsr`, `vca`, `svf`, ported from `m2-dsp`.
   - CLI: `music2 build` and `music2 render`.
   - Demo: a bass track renders to a WAV.
   - Benchmark: WebAssembly kernels against native `m2-dsp`.
2. **Projects:** timeline data files, `tx`, undo, `music2.toml`/`music2.lock`, git package resolution, `std` resolved as a package.
3. **Many modules:** `import`, cycle diagnostics, latency compensation, `feedback`, plus `std/mix`, `std/arrange`, `follow`/`duck`.
4. **Real time:** hot swap with stable ids, audio-device playback in the desktop app, file watching.
5. **The agent surface:** MCP generated from the core API, `render` taps, `std/meters`, the analysis tool and spectrograms, variants, locks.
6. **A complete std:** port every current instrument and effect. Retire `m2-graph`, `m2-render` and the old `m2-engine` commands.
7. **The timeline UI on the new core:** reuse the arrangement and piano roll, add a lane for each declared lane.
8. **The node editor:** read-only first, then editing the canonical subset, then group nodes.
