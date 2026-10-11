# Song as a program

A design for music production as a software engineering problem.

Status: proposal (2026-10-11). Nothing here is implemented yet.

## The idea in one sentence

**A song is a program whose only input is time.** You describe it in TypeScript, a compiler checks it, and a native engine plays it.

Blender's node editor is the model for how this should feel. Each track is a node graph:
- The **inputs** are notes, automation curves, masks and the clock.
- The **output** is sound for the mix, or any other signal that other tracks can use.
- **Devices** are nodes, and the **track** is the graph that connects them.
- There is **no per-clip change to the graph**. Per-clip behaviour is expressed with masks.

## Goals

- **The graph is the source of truth for behaviour.** What you hear is fully determined by the graph, the timeline data and the clock.
- **Every connection is typed.** Type errors are caught before anything plays: audio vs control vs events, and units such as Hz vs dB.
- **References between tracks are explicit and checked.** Dependencies have a known processing order, and a cycle is a compile error.
- **It is ordinary software.** It diffs in git, can be split into reusable modules, can be tested, linted and built incrementally.
- **A GUI node editor and the code are two views of the same thing.** Neither one is a second-class citizen.

### Non-goals for the first version

- Users writing their own sample-level DSP. The first version ships a built-in node library.
- Hosting VST/AU plugins.
- Recording live audio.

## The six primitives

### 1. Clock

The clock is the one thing everything shares.

- There is a single **root clock** measured in beats.
- The **tempo map** is one song-level function from beats to seconds. Tempo is not a signal that nodes can modulate, because that would let time depend on itself.
- Nodes can **derive** clocks: `clock.every("1/8")`, `clock.swing(0.6)`, `clock.div(3)`, `clock.offset("1/16")`. Polyrhythms, swing and other rhythmic grids are all derived clocks.
- Every derived clock comes from the root. That keeps the invariant: there is exactly one notion of "now".

```ts
type Clock = { readonly __kind: "clock" };   // opaque; derived via methods
```

Time has three domains: `Beats`, `Seconds` and `Samples`. They are separate branded number types, and conversions between them are explicit and go through the tempo map and the sample rate.

### 2. Signal

A signal is a value that changes over time. There are three kinds:

| Kind | Meaning | Rate | Examples |
|---|---|---|---|
| `Audio<Ch>` | sound | sample rate | oscillator output, a track's `out` |
| `Control<U>` | continuous value with a unit | block rate, interpolated to the sample | automation, LFO, envelope follower |
| `Events<T>` | discrete things at instants or spans | exact to the sample | notes, triggers, chords |

**Units are part of the type:** `Hz`, `dB`, `Gain` (linear), `Semitones`, `Seconds`, `Beats`, `Unit01` (0..1) and `Bipolar` (-1..1).
- Connecting a `Control<Hz>` to a `Control<dB>` input does not compile.
- Conversions are explicit: `toHz(semitones)`, `dbToGain(x)`.
- Unit literals are values: `800*Hz`, `-6*dB`.

**A mask** is `Control<Unit01>`, and **a gate** is a mask that is only ever 0 or 1. Masks are what replace per-clip graph edits:

```ts
blend(a, b, mask)        // crossfade between two signals
when(mask, signal)       // signal × mask
```

**A note** is an event that carries its own curves:

```ts
type Note = {
  pitch: Control<Semitones>;   // includes per-note bends (MPE)
  velocity: number;            // 0..1
  gate: Control<Unit01>;       // 1 while held
  pressure: Control<Unit01>;
  timbre: Control<Unit01>;
};
```

### 3. Node

A node is a typed function from signals (and constant params) to signals.

Every node must be:
- **Causal.** Its output at time `t` depends only on its inputs up to `t`, plus any **look-ahead** it declares (in samples). A look-ahead limiter is the typical example.
- **Deterministic.** The same inputs, the same sample rate and the same start state always give the same output. Randomness takes an explicit seed.
- **Latency-declaring.** The compiler adds up latencies along each path and inserts compensating delays automatically (plugin delay compensation).

Nodes can hold state (filters, delays, reverbs), but that state is internal and deterministic.

Those rules make the following possible:
- Any time range can be rendered on its own, by pre-rolling from the nearest snapshot or from 0.
- A render is cached under a hash of its node, its inputs and its params. An edit only invalidates what is downstream of it.
- Rendering offline and playing in real time give exactly the same output.

### 4. Lane

**A lane is a typed hole in a track's graph that the timeline fills.**

```ts
lane.notes("notes")                       // Events<Note>
lane.control("cutoff", Hz, 800*Hz)       // Control<Hz>, default when no clip covers t
lane.mask("drop")                         // Control<Unit01>, default 0
lane.audio("vox")                         // Audio, from placed audio clips
```

**A clip is data on a lane over a time range.** Clips are never code. The rule:

- **Code is behaviour**, like a shader.
- **Clips are data**, like a mesh or a texture.

Where clips overlap on a control lane, the later one wins. Gaps between clips use the lane's default value. Each audio clip is played through the lane with its own gain and fades.

### 5. Track

A track is a **module**. It contains:
- a graph
- the lanes it declares
- named **exports**

```ts
export const bass = track("Bass", ({ lane, clock }) => {
  ...
  return { out, env: follow(out), sends: { verb: out.mul(-12*dB) } };
});
```

- `out` is the export the mix uses. Any other export can be used by other tracks.
- A **bus** is a track whose graph pulls in other tracks' exports. It does not have to declare lanes.
- **Mute** and **solo** are masks applied to a track's `out` at the mix. They are part of the song state, not the graph.
- Tracks are the units the UI works with: they get a mixer strip, rows in the arrangement and solo. When the song is compiled, all tracks are flattened into one graph.

### 6. Song

The song is the root module. It has:
- `tempo`: the tempo map.
- `sections`: arrangement markers such as `intro:8` and `verse:16`, measured in bars. **Every section is a global mask**: `section("chorus")` returns a gate, so the arrangement is something the code can respond to.
- `mix`: the final sum, i.e. the master. It is the root of the dependency graph, and whatever it doesn't reach is not computed.

```ts
export default song({
  tempo: tempoMap(124),
  sections: ["intro:8", "verse:16", "drop:16"],
  mix: { kick: kick.out, bass: duck(bass.out, kick.env, { depth: -8*dB }) },
  master: chain => chain.pipe(glue(), limiter(-1*dB)),
});
```

## Polyphony: an instrument is a function over one note

```ts
const voice = (n: Note) => saw(n.pitch).mul(adsr(n.gate, { a: 0.005, d: 0.2, s: 0.6, r: 0.1 }));
const synth = poly(notes, voice, { voices: 16, steal: "oldest" });   // Audio
```

`poly` runs the voice graph once for each active note and sums the results. Per-note expression (MPE) comes for free, because every note carries its own curves.

A monophonic instrument is `mono(notes, voice, { legato: true, glide: 30*ms })`.

## References between tracks

**The graph only pulls.** A track that uses a signal references the track that produces it, by importing it. Nothing pushes.

```ts
// sidechain
duck(bass.out, kick.env)

// reverb bus pulls sends
export const verb = track("Verb", () => ({ out: reverb(sum(vox.sends.verb, keys.sends.verb)) }));
```

The compiler handles dependencies as follows:
1. Build the dependency graph between tracks from these references. Tracks are module boundaries, and each export is an edge.
2. Sort it to get the processing order.
3. **Treat any cycle as a compile error.** The error names the loop: `bass.out → verb → bass (via sidechain)`.
4. Allow intended feedback only through an explicit `feedback(signal, { delay })`. The delay is at least one processing block. This matches how real DSP feedback works.

Because only pulling is allowed, every dependency is visible on the consumer's side. In the GUI, a "send" knob on the vocal track is just a view of the edge that the bus pulls.

## TypeScript describes the graph; it does not run in it

TypeScript cannot run on the audio thread, so authoring and playback are **separate stages**. This is similar to how JAX traces a function, or to Elementary Audio.

```
song.ts ──tsc──▶ typed? ──run builder──▶ graph IR (JSON) ──engine──▶ validated, compiled ──▶ audio
          types, kinds, units      lanes, nodes, exports     cycles, latency, rates
```

1. **`tsc`** checks kinds, units and that exports exist.
2. **The builder** runs the module. Calls like `saw(...)` don't make sound. They record nodes and return handles to them. The result is the **IR**.
3. **The engine** (Rust) checks the IR again rather than trusting it. It then flattens the graph, sorts it, assigns buffers, inserts latency compensation and produces a processing plan.
4. **Hot reload:** the new plan is swapped in at the next block boundary. Any node whose **stable id** matches a node in the old plan keeps its state, so reverb tails don't cut off and held notes keep sounding.

**Stable ids** come from the call path: track name, then variable binding, then call position. You can override one with `.id("name")`. These ids play the same role as keys in React.

### Sketch of the IR

```json
{
  "version": 1,
  "tempo": { "segments": [{ "at_beat": 0, "bpm": 124 }] },
  "sections": [{ "name": "intro", "bars": 8 }],
  "tracks": [{
    "name": "Bass",
    "lanes": [
      { "id": "notes", "kind": "events/note" },
      { "id": "cutoff", "kind": "control", "unit": "Hz", "default": 800 }
    ],
    "nodes": [
      { "id": "bass/voice", "op": "poly", "inputs": { "events": "lane:notes" }, "body": "graph:bass/voiceFn", "params": { "voices": 16 } },
      { "id": "bass/f", "op": "lowpass", "inputs": { "in": "bass/voice.out", "cutoff": "lane:cutoff" }, "params": { "q": 0.7 } }
    ],
    "exports": { "out": "bass/f.out" }
  }],
  "mix": { "inputs": { "bass": "track:Bass.out" }, "master": [] }
}
```

`m2-graph` already defines plugins as data. It is the natural seed for this IR, and its node library is the seed for the built-in nodes.

## Project layout on disk

```
my-song/
  song.ts            # root module: tempo, sections, mix
  tracks/
    bass.ts          # one module per track (by convention)
    kick.ts
  lib/               # your own reusable chains/instruments
  timeline/
    bass.notes.json  # clip data per lane, diff-friendly (one clip per line/object)
    bass.cutoff.json
  assets/            # audio files, content-addressed
  layout.json        # node positions for the GUI; never affects sound
  tests/
    mix.test.ts
```

Behaviour (`.ts`), data (`timeline/`, `assets/`) and presentation (`layout.json`) are kept separate, so diffs are about one thing at a time.

## How the GUI and the code stay in sync

**The TypeScript is the source of truth.** The node editor reads it and writes it back.

- **Reading:** the editor draws nodes from the IR and places them using `layout.json`. Each IR node points back to its location in the source.
- **Writing:** a GUI edit becomes a **small, targeted change to the code**, e.g. "set param `q` of `bass/f` to 1.2" or "insert `delay()` between `x` and `y`". The edit is applied to the syntax tree, and formatting and comments are kept.
- **Code the GUI can't draw**, such as loops, conditionals and helper functions, is shown as a collapsed **group node** with its typed ports. You can open it to see the code. This is how node groups work in Blender.
- **Canonical subset:** code that sticks to straight-line `const x = node(...)` statements can be fully edited in the GUI. Anything beyond that still works, but the GUI can only change it through the group node's ports.

## What the software-engineering framing buys you

- **Version control:** songs diff and merge as text. Clip data is stored in a diff-friendly format.
- **Packages:** a vocal chain, a drum bus or a synth is an exported function. You share it the way you share code.
- **Tests:** render a range and assert on the result.
  ```ts
  test("drop doesn't clip", () => expect(render(song, section("drop"))).toPeakBelow(-1*dB));
  test("intro is quiet", () => expect(render(song, section("intro"))).toLoudnessBelow(-18*LUFS));
  ```
  Snapshot tests compare a render against a stored hash.
- **Lint rules** run on the IR together with the timeline data: notes outside the song key, a bass overlapping the kick in the low end with no ducking, a track nothing listens to, a mask lane that is never used.
- **Incremental builds:** renders are cached by content hash, so an edit only re-renders what is downstream of it. Freezing a track is the same as keeping its cache.
- **Scripting and agents:** the IR and the timeline are plain data. A local Claude session can read and write them through the same commands the GUI uses.

## How it maps onto the music2 codebase today

| Today | In this design |
|---|---|
| Track = clips + device chain | Track = module (graph + lanes + exports) |
| Device / plugin instance | Node, or a group node from `lib/` |
| Clip-level automation and settings | Clips on lanes, plus masks |
| Sends and buses | Pull references to exports |
| `m2-graph` plugin definitions | The seed for the IR and the built-in node library |
| Engine command registry | GUI edits as code changes, plus timeline data commands |
| `m2-render` | The plan executor (offline and real-time) |

## Risks and open questions

1. **Changing the graph from the GUI.** Targeted code changes are reliable on the canonical subset but fragile beyond it. The group-node fallback has to feel good, not like a dead end.
2. **The cost of `poly`.** A separate voice graph per note is elegant but has to be fast. Plan: compile the voice body once and run all voices together.
3. **Users writing their own DSP.** The options for later versions are a Faust-like sub-language, AssemblyScript to WASM, or Rust nodes. The choice can wait, but the IR should leave room for an `op: "wasm"` node.
4. **VST/AU plugins** would be opaque nodes with declared ports and latency. They are only roughly deterministic, so caching must treat them as impure.
5. **Tempo as a signal.** It is excluded on purpose for now. Tempo ramps are allowed as segments of the tempo map.
6. **Real-time input** (playing live MIDI, recording) is new data arriving on a lane. The data is still deterministic once recorded, but monitoring needs a low-latency path that skips the cache.
7. **Sample-rate changes and render length.** The tail after the last section ends should be computed from the reverb and delay tails in the graph.

## Prior art

| Tool | What to learn from it |
|---|---|
| Elementary Audio | JavaScript builds the graph and native code renders it. Closest to the staged design here. |
| Faust | A functional DSP language that compiles to fast code. |
| SuperCollider, Max/MSP, Pure Data | Patching as programming. |
| Bitwig Grid, Reaktor | Modular graphs inside a DAW. |
| TidalCycles, Strudel | Patterns as values. |
| Blender geometry nodes | Group nodes, a node editor kept in sync with data, caching. |

None of these combines a typed **whole-song** compile with a timeline of lanes, arrangement masks and checked references between tracks. That combination is what this design adds.

## Milestones

1. **A thin end-to-end slice.**
   - `@music2/sdk`: types, units, the builder and a few nodes (`saw`, `sine`, `adsr`, `lowpass`, `gain`, `poly`).
   - The builder outputs IR JSON.
   - Rust checks the IR and renders a WAV of one track: notes lane → `poly(saw)` → lowpass driven by an automation lane.
   - Shown with a CLI: `music2 build song.ts && music2 render out.wav`.
2. **Multiple tracks:** exports, references between tracks, cycle errors with readable messages, latency compensation, `duck`/`follow`.
3. **Sections and masks:** `section()`, `blend`, `when`, mute and solo as masks.
4. **Hot reload:** stable ids, state carried across a swap, real-time playback in the desktop app.
5. **Tests and lint:** `render()` in tests, loudness and peak assertions, a first set of lint rules.
6. **The node editor:** read-only first, then editing the canonical subset, then group nodes.
7. **The timeline UI** on top of the lanes, reusing the current arrangement and piano roll.
