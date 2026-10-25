# Brief: two-way editing between code and graph

> **In short:** history: the request as sent on 2026-10-11. Its "today" sections describe the code at that date; see [code-graph-editing-design.md](code-graph-editing-design.md) for the design that answered it, and the [architecture contract](architecture.md)'s *Today* lines for what exists now. Its code and paths are in the format of that date, since replaced: tracks now take a key first (`track("bass", { name: "Bass" }, fn)`), and lane data lives in `data/<track>/<lane>.json` ([Project format v1](architecture.md#project-format-v1)).

*For a specialist asked to design the implementation. Written 2026-10-11.*

## The ask

music2 is a music production app in which **a song is a TypeScript program** that builds an audio graph. I want the relationship between that code and the app's **node editor** to work like **SwiftUI code and Xcode's canvas**:

- **The code is the single source of truth.** Nothing about how the song sounds lives anywhere else.
- **The node editor is a live view of the code that can also edit it.** Dragging a knob, connecting a cable or adding a node changes the code, minimally and cleanly, as if a careful person had typed it.
- **Editing the code updates the graph immediately.** This holds whether the edit comes from a text editor, a coding agent, or the app.
- **For an artist, the graph should feel like a first-class instrument,** not a viewer bolted onto a text file. Knobs must feel instant.

**Please design the best implementation of this relationship:** the house style of code that makes it work, how nodes and arguments are identified, how GUI gestures become code edits and how those edits are verified, how it stays fast, and how it behaves when several people or tools edit at once. Please challenge my current thinking (below) where you see something better.

---

## Background

### What music2 is

A DAW (in the spirit of Ableton Live or Logic Pro) aimed at professional producers. The longer-term goal is that artists like A. G. Cook make records with it, using open-source instruments and effects that others publish, and that AI agents can work on a song through the same interfaces as people. The full design is [`docs/song-as-program.md`](song-as-program.md). The essentials for this problem:

- **Graphs are code; the timeline is data.**
  - Each **track** is a TypeScript module that builds a graph of nodes: oscillators, filters, effects, instruments, mixers.
  - Notes, automation curves, audio clips and the tempo map live in **timeline data files** (JSON). They are never in code.
  - Hosted plugins (CLAP/VST3/AU) keep their state in data files that the code refers to by name.
- **Signals are typed:** `audio<1|2>`, `control<unit>` (with units such as `hz`, `db`, `seconds`, `semitones`, `unit01`), and `events<note>`. A compiler in Rust checks every connection.
- **Lanes connect code to the timeline:** `lane.notes("notes")` and `lane.control("cutoff", "hz", hz(500))` declare inputs that the timeline fills.
- **`poly(notes, voice => …)`** builds one copy of a voice graph per sounding note.
- **Tracks read each other's outputs** by importing them: `duck(bass.out, kick.env)`.
- **Instruments and effects are ordinary functions** from packages (the standard library `std` is one), for example `bass(notes, { cutoff })` or `svf.lowpass(x, hz(800))`. A kernel is a unit of compiled DSP (WebAssembly) with typed ports.

### What a song looks like today

This is real code from [`examples/first-song`](../examples/first-song), which builds and renders.

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
// song.ts: the root module; its result is what you hear
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
      bass: duck(bass.out, kick.env, { depth: db(-9) }), // sidechain
      keys: { in: keys.out, db: db(-5), pan: -0.25 },
    }),
  ),
);
```

`bass()` itself lives in the std package and is ordinary code built from smaller nodes:

```ts
export function bass(notes: Events, o: { cutoff?: Value; drive?: Value; voices?: number } = {}): Audio {
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
}
```

### How code becomes a graph today

1. **Sandbox.** Each `.ts` file is stripped of types by **oxc** (with source maps) and run in **QuickJS** inside the Rust process. The sandbox has no file system, network, clock or randomness. Imports are limited to project files, `@m2/core` and installed packages.
2. **Recording.** Calls into the SDK (`@m2/core`: `kernel()`, `poly()`, `lane.*`, `track()`, …) don't make sound. They **record nodes** into the module being built. Plain numbers and unit values (`hz(800)`) become `const` nodes.
3. **IR.** The result is a JSON graph, the IR. Each node is one of a few ops: `lane`, `time`, `const`, `kernel`, `poly`, `feedback`, `import`, and later `plugin`.
4. **Checking.** A Rust checker validates the IR independently (kinds, units, cycles, imports) and flattens it into a runnable plan.

What the IR records about the source today, from the real `Kick` module (abridged):

```json
{ "name": "Kick", "track": true,
  "lanes": [{ "id": "hits", "kind": "events<note>" }],
  "nodes": [
    { "id": "n0",  "op": "lane", "lane": "hits", "src": "tracks/kick.ts:6:25" },
    { "id": "n24", "op": "poly", "events": "n0", "voices": 2, "body": { "nodes": ["…13 nodes…"], "out": "n23" }, "src": "tracks/kick.ts:6:25" },
    { "id": "n25", "op": "const", "value": 0.15, "unit": "seconds", "src": "tracks/kick.ts:8:45" },
    { "id": "n26", "op": "kernel", "kernel": "std:follow", "inputs": { "in": "n24", "release": "n25" }, "src": "tracks/kick.ts:8:45" }
  ],
  "exports": { "env": "n26", "out": "n24" } }
```

**Note what is missing for editing:**
- **Node ids are build-order counters** (`n24`), so they change when code above them changes.
- **`src` is only the call site of the first stack frame in the project.** Nodes created inside a library function all point at the user's call to that function. The constant `0.15` points at the `follow(...)` call, not at `ms(150)`.
- **No argument spans or binding names** are recorded.

All of this is open to change; the SDK, IR and checker are ours.

Measured on the developer's Mac: a full `m2 check` of the four-track example takes about **70 ms** end to end. That includes process start, compiling the kernels, running all modules and checking.

### Who edits a song, often at the same time

- **The artist in the app:** node editor, timeline, mixer.
- **The artist in a text editor:** VS Code, with the file open while the app runs.
- **Coding agents:** for example Claude Code editing files directly, or an agent calling the app's API.
- **The app itself:** for example "automate this knob" writes code and creates a timeline lane.

The planned session layer has revisions, atomic transactions over code and timeline files, undo, and a **draft versus accepted** split. Files on disk are the draft, and the engine plays the last revision that compiled. Your design should fit that model, or tell us how to change it.

---

## Requirements

1. **Code is the source of truth.** The graph is always derived from code. Node positions and similar layout live in a sidecar file (`layout.json`) that never affects sound.
2. **Gestures become minimal text edits.** A knob change touches only the characters of that value. Formatting, comments and unrelated lines are untouched. Diffs read as if a person made them.
3. **Every GUI edit is verified.** After applying an edit, rebuilding must give exactly the intended graph change and nothing else. If it doesn't, the edit is not applied and the user sees why.
4. **It feels instant.**
   - While dragging a knob, sound changes within a few milliseconds, with no rebuild. The code is updated when the gesture ends.
   - Structural edits, such as adding a node or rewiring, are audible within roughly 100–200 ms on a typical track.
5. **Identity is stable.** A node keeps its identity across edits, including edits made in a text editor. Layout, selection, undo history and the audio engine's running state (reverb tails, held notes) all key off it.
6. **Unusual code degrades gracefully.** Code the graph can't edit (loops, helper functions, library instruments) still displays sensibly, as a group with editable arguments where possible. It is never corrupted, and the user can always tell what is editable.
7. **Concurrency.** Edits from the app, a text editor and an agent can overlap. Nothing is silently lost. The app copes with a file that doesn't currently compile (someone mid-typing) by showing the last good graph plus the error.
8. **Undo.** One gesture is one undo step. App undo and text-editor undo shouldn't fight.
9. **Edits across files.** Some gestures touch more than one file:
   - connecting a cable from the kick track to the bass track adds an import and an export;
   - renaming a lane must also rename its timeline data file.

   These must be atomic.

## The gestures to support

| Gesture in the graph | Expected effect in code |
|---|---|
| Turn a knob | change one literal (`0.3` → `0.45`, `hz(800)` → `hz(950)`) |
| Turn a knob that is at its default (argument omitted) | insert the argument |
| Reset a knob to its default | remove the argument (or not: your call) |
| Connect a cable to an input | set that argument to a binding's name |
| Disconnect a cable | revert the argument to a literal or the default |
| Add a node from the palette (a kernel, an instrument, a plugin) | insert a `const` binding (and an import if needed) |
| Delete a node | remove the binding and any references to it |
| Rename a node | rename the binding everywhere it is used |
| Automate a knob | `0.3` → `lane.control("mix", "unit01", 0.3)`, plus a timeline lane |
| Stop automating | the reverse, keeping the last value |
| Expose a signal to other tracks / connect across tracks | add an export in one file, an import and a reference in another |
| Group selected nodes into a reusable module | extract a function or a module file (stretch goal) |
| Move a node | `layout.json` only |
| Open a group | navigate to the code (or project module) it comes from |

## Hard cases, please address each

1. **Unnamed nodes.** `vca(svf.lowpass(x, hz(800)), env)` creates a filter with no binding name. How is it identified, and what happens when the user drags its cutoff?
2. **How values are written.** A value can be:
   - a plain number: `0.3`, `-12`;
   - a unit call: `hz(800)`, `khz(2.5)`, `ms(30)`, `db(-6)`;
   - an expression: `2 * 440`, `scale(cutoff, 4)`;
   - a reference to a named constant.

   Which of these can a knob edit, and what does dragging `khz(2.5)` down to 900 Hz write?
3. **Defaults and options objects.**
   - Omitted arguments, like `adsr(gate)` with no options.
   - Spread options: `kernel("std:adsr", { gate, ...o })`.
   - Fallbacks: `o.cutoff ?? hz(600)`.
4. **Library instruments.** `bass(notes, { cutoff })` creates about 25 nodes inside std code. The user should see one group whose ports are `bass`'s options. Where does the knowledge of those options, their kinds and their defaults come from: TypeScript types, metadata, or recorded calls?
5. **`poly` voice bodies.** In `poly(notes, n => …)` the arrow function is a nested scope. Should its insides be editable as a sub-graph? Cables from outer signals into the voice are captures.
6. **Statements that aren't bindings.**
   - `feedback("audio<1>")` followed by `fb.set(x)`.
   - The `return { out, env: follow(out) }` object literal.
   - Nested object literals used as data, like the `mixer({ hats: { in: hats.out, db: db(-6), pan: 0.3 } })` arguments above.
7. **Code generated in loops.** For example `for (const f of [100, 200, 400]) bands.push(svf.bandpass(x, hz(f)))`. Can any part be edited (say the `400` in the array), or is it all one group?
8. **Edits from outside the app.** If someone renames `cutoff` to `bright` in VS Code, does the node keep its layout and running state? Is rename detection worth it?
9. **Overlapping edits.** An agent edits `bass.ts` while the user is mid-drag on a knob in it.
10. **Invalid intermediate code.** The file doesn't parse because the user is mid-typing. Can the graph still be edited, or is it read-only until the code compiles?
11. **Lanes are a contract with data files.** A lane's id names its timeline file (`timeline/bass.cutoff.json`). Renaming a lane from the graph must rename the file in the same transaction.
12. **Formatting.** Should the project enforce a formatter (prettier-like) so inserted code always matches, or should edits adapt to each file's existing style?

---

## Current thinking: a starting point, not a mandate

1. **A declarative house style.** A track is a list of named `const` bindings and a `return`, and every gesture maps to a small text change (the gestures table above).
2. **Binding names are node identity.** `cutoff` is the node. Layout and hot-swap state are keyed by it.
3. **The compiler records exact spans** for every node and every argument, so "this knob" means "these characters".
4. **Every GUI edit is verified by the compiler.** Patch, rebuild, compare against the intended graph change, and roll back if they differ. The control is then shown as not editable from the graph.
5. **Code outside the house style is a sealed box:** a group whose arguments can be edited but whose insides can't, unless it is project code you can open.
6. **Knob turns take a fast path.** Values go straight to the running engine while dragging, and the literal is written when you let go (debounced).

Parts I'm unsure about:
- whether binding names are robust enough as identity;
- whether to trace the JavaScript's execution or analyse the source statically to map nodes to text;
- how much of the house style a linter should enforce;
- how a group's ports should be described.

## Prior art worth weighing

- **SwiftUI and Xcode previews/canvas.** The model, including where its canvas editing gives up.
- **Sketch-n-Sketch, and research on bidirectional programming and output-directed editing.** The closest academic work.
- **Lenses and bidirectional transformations.**
- **Max/MSP** (codebox and `gen~`), **Houdini** (VEX inside VOP networks), **Blender** (Python and geometry nodes).
- **Unreal Blueprints** (graphs as the only source), as a contrast.
- **Figma's code-connect tooling.**
- **Structured and projectional editors** (JetBrains MPS, Hazel).
- **Code modification toolkits** that preserve formatting: recast, ts-morph, oxc's own codegen and its span-preserving edits.

## Constraints and existing pieces

- **Languages:** the engine and compiler are Rust. The app UI is React + Tailwind in a Tauri desktop shell (macOS first).
- **Tooling already in the stack:** oxc (parser, transformer, codegen and source maps) and QuickJS (via rquickjs) are linked into the Rust process. Prefer using them over adding Node.js.
- **The sandbox stays.** Song code runs with no file system, network, clock or randomness. Builds must stay deterministic.
- **The checker in Rust is authoritative.** `tsc` is optional editor tooling only.
- **Must work for agents too.** An agent should be able to request "connect X to Y" as a structured operation and get the same verified code edit a GUI gesture would.

## What I'd like back

1. **A design document** answering the requirements and every hard case, with your recommendations and the alternatives you rejected.
2. **The house style**, written as rules we could put in a linter, and what happens when code breaks them.
3. **The identity scheme**, and the changes needed in the SDK, the IR and the compiler (for example argument spans, binding names, stable ids).
4. **The edit-operation API** between the GUI (or an agent), the session and the compiler: message shapes for gestures, verification results, conflicts and errors.
5. **A prototype** on [`examples/first-song`](../examples/first-song). For `tracks/bass.ts` and `song.ts`, implement turning a knob, connecting and disconnecting, adding and deleting a node, and automating a knob. Each must produce a minimal, compiler-verified code edit.
6. **A test suite** built around one property: for every editable element in a body of house-style files and every gesture, the edit rebuilds to exactly the intended graph change; no text outside the edited spans changes; comments survive. Include adversarial inputs.
7. **Timings** for the fast path, structural edits and verification on the example project.
8. **Open problems** and what you would do next.

**Success looks like this:** an artist edits `tracks/bass.ts` only by dragging in the graph, then opens the file and finds code they could have written by hand, with a clean diff. And an edit typed in VS Code shows up in the graph immediately without the graph losing track of anything.

## Out of scope

- Visual design of the node editor itself.
- Timeline, piano roll and audio clip editing, which are data edits, not code edits.
- Plugin hosting and the realtime audio engine, except for the fast-path value update.

## Getting oriented in the repository

| What | Where |
|---|---|
| The overall design | `docs/song-as-program.md` |
| The SDK every song imports (node recording, `src` capture, lanes, `poly`, tracks) | `core/script/sdk/core.ts` |
| The sandbox (oxc stripping, source maps, QuickJS, module resolution) | `core/script/src/lib.rs` |
| The IR types | `core/ir/src/lib.rs` |
| The checker (flattening, kinds, units, cycles) | `core/ir/src/check.rs` |
| The compile step every host uses | `core/session/src/lib.rs` |
| The standard library's TypeScript (the instruments and wrappers above) | `std/src/*.ts` |
| The example project | `examples/first-song/` |

Useful commands, run from the repository root:

```bash
cargo test --workspace                                         # everything (about 64 tests)
cargo run --release -p m2 -- check examples/first-song          # build and check
cargo run --release -p m2 -- check examples/first-song --json   # print the IR
cargo run --release -p m2 -- render examples/first-song out.wav
```
