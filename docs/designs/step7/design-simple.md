# Step 7 design: the engine, simplest that passes

Against `docs/architecture.md` (normative) at HEAD `cbf5eb9`. Built on the three understanding notes in
this folder (`understand-runtime.md`, `understand-identity.md`, `understand-contract.md`). Read-only
survey: nothing was built or run. Every cost marked *estimate* is unmeasured; G7f and the slice 1
measurements replace them.

Step 7's line in the order of work:

> `Plan` split from `DataSet`; SiteId matching, InstancePath and RuntimeId. The arena, with prepare and
> activate, keeping instances whose RuntimeId and shape (wasm hash included) match. Arrangement switches
> prepared off the audio thread; persistent CLAP instances behind `PluginHost`, with `LoadState`, and
> offline renders that reuse them.

Gate:

> In p50, a structural edit at 3:00 is audible within 200 ms after the compile, and the tails of
> untouched tracks are intact. Live playback with no edits is bit-identical to offline. A kernel module
> rebuilt with the same kernel id but a new hash starts from init. An export of `examples/with-plugins`
> pauses playback, reuses its instances, and playback resumes with their state intact. The per-instance
> `Store` cost at 6,176 instances is measured, and the store layout is chosen from it.

The angle: the smallest set of concepts that meets every gate. Reuse the identity step 6 already built,
make one mechanism carry every kind of state across a plan change, and delete what that mechanism
replaces.

---

## 0. The design on one page

**One rule:** every stateful node of a plan holds its instance in a box. An activation moves boxes from
the plan that plays into the new plan, by a move list computed off the audio thread. Whatever isn't
moved is retired with the old plan. That one rule covers all of these:

- kernel instances;
- `poly` voice pools;
- plugin instances;
- feedback rings;
- audio-lane fade taps;
- notes-lane schedulers.

It replaces step 3a's whole-renderer swap and step 3b's plugin carry.

**What is new:**

| Concept | What it is | Where |
|---|---|---|
| `Plan` | the structure prepared from a `Flat`: exec wrappers, buffers, order, cells, fades | `core/runtime/src/plan.rs` (new) |
| `Lanes` | what the plan's nodes read from the `DataSet`: notes schedulers (which hold state) and control lanes | `core/runtime/src/lib.rs` |
| instance box | the part of a stateful node that survives a plan: `Box<KernelInst>`, `Box<VoicePool>`, `Arc<PluginInstance>`, a ring, the audio taps, a scheduler | `Exec` variants |
| `Shape` | what must be equal for a box to move: kernel id + package wasm sha + channels, and so on | `plan.rs` |
| continuation | for each top-level node of the new `Flat`, the old `Flat` node it continues, found by step 6's delta names | `core/session/src/tx/continues.rs` (new) |
| `Entry` | the move list and what the audio thread does at the block line, prepared | `plan.rs`, carried inside the new `Renderer` |
| ghost | an old node that is replaced or removed and still feeds a fade: it runs in the new plan for one 5 ms window with its old instance, then is retired | `plan.rs` |
| `Exec::Fade` | a tiny node on each changed edge into a kept node (and on the song output), fading old source to new over 5 ms, exact when the two are equal | `lib.rs` |
| `PlanInfo` | an immutable server-side record of what a plan holds (shapes, lanes, plugin instances, names), so the next prepare never touches the live plan | `plan.rs` |

**What doesn't exist, on purpose:**

- no `SiteId(u32)` counter: a site is a point in the comparison's coordinates (ID-13 edit);
- no `RuntimeId` type and no `Map<RuntimeId, Slot>`: the delta name is the RuntimeId, it never leaves
  the session, and the arena is the playing plan's own slots;
- no `LoadState` or `SetArrangement` engine messages: a state load is a main-thread host call, and an
  arrangement switch is an `Activate` of the same structure;
- no second renderer during a fade, and no lending: no instance is ever processed twice.

**The engine message set stays as it is today:** `Activate { renderer }`, `SwapLane`, `Transport(Cmd)`,
`Panic`.

**Export:**

1. The Live is taken out of the device callback with today's 5 ms `LiveFade`, so playback is paused and
   frozen.
2. An offline plan is built with fresh kernels and the live plugin instances, shared by `Arc` and found by
   the same continuation match.
3. Each instance goes through RENDER-1's host calls, then the render runs on the export thread.
4. The calls are reversed, and the Live is handed back with a 5 ms fade-in.

**The Store:** one wasmtime `Store` per kernel instance. A `Store` never frees an instance until the
`Store` itself drops, so a shared store would leak every replaced instance, and an instance in a shared
store can't move. G7f measures the cost first (slice 1). Its fallback is the pooling allocator, with the
same code shape.

**Build order** (4 slices, each green):

1. Measure the Store, and make instances movable. A refactor with no change in behaviour.
2. The arena: keep, ghost, fade, persistent schedulers and arrangement switches. Uses step 6's names as
   they are.
3. Names that survive edits: token diff, `(` points, ComponentId, `key()` cut, ID-14 counts.
4. Plugins persist: `LoadState`, and export that pauses, borrows and resumes.

**Deleted:**

- in the runtime: `Carry`, `CarryKey`, `InstanceSource`, `Fresh`, `carry_from`, `lend`,
  `render_block_lent`, the engine's `Fade` and `fading`, the coalescing of activations;
- in the scheduler: `NoteScheduler::adopt`, `raise_next_id`, `next_id`, `released`, `adopted` and
  `release_all`;
- in plugins: `PluginInstance.{key, held_cap, next_id}`, `PluginNode.lent`, and the runtime's
  plugin-in-voice branch;
- in lanes: `LaneSlot::Audio`;
- in the server: `Open.carry` and the `Carry` plumbing;
- `m2_ir::control_id`.

Section 11 lists every item.

---

## 1. Concepts, and the guarantees they keep

### 1.1 What a producer must not notice (unchanged by this step)

| Guarantee | Rule | How this design keeps it |
|---|---|---|
| No clicks at an activation | RENDER-25a | a 5 ms fade on every changed edge into a kept node, and on the song output; G6 `step` stays a gate |
| No stuck notes | RENDER-13, 26a | schedulers persist, so a kept consumer keeps its notes; a replaced consumer, or a kept one whose lane changed, gets every note off at entry |
| Tails of untouched tracks intact | RENDER-25 | their instances move; nothing upstream of them changed, so no edge of theirs fades |
| Export exact and deterministic | RENDER-1, 2 | offline plans are prepared fresh exactly as today; borrowed plugins are reset and loaded with the committed blob |
| Live equals offline | RENDER-33 | the same `Graph::process`; an activation that keeps everything changes no bit (new test) |
| No allocation, lock or free on the audio thread | RENDER-28 | every list the entry needs is sized at prepare; replaced objects are retired; names and hashes never reach the audio thread |
| No jump back | RENDER-8 | the transport is copied and retimed at entry, as today |

### 1.2 Stateful nodes and their boxes

The arena holds these, and only these (`Exec` in `core/runtime/src/lib.rs`):

| `Exec` variant | Box (moves at entry) | Plan side (new each plan) | Shape |
|---|---|---|---|
| `Kernel(KernelNode)` | `Box<KernelInst>`: its own `Store`, `Memory`, the six `TypedFunc`s, port pointers, out channels | `inputs: Vec<Input>`, `seen: Vec<u64>`, `consts_written: bool` | `Kernel { id, wasm: Sha, channels }` |
| `Poly(PolyNode)` | `Box<VoicePool>`: `voices: Vec<VoiceSlot>` (each with its voice `Graph`, kernel instances, note, `born`, `dirty`, `last_peak`), the pool's `born` | `lane: usize`, `out`, `captures: Vec<PortRef>`, `plan: Vec<Planned>` (scratch sized by caps), flags | `Poly { voices, channels, voice: bare Voice, wasm: [Sha] }` |
| `Plugin(PluginNode)` | `Arc<PluginInstance>` | `audio_in`, `notes`, `params` (last-sent `None`), event lists, `keys`/`ended`/`release` (sized by caps, copied in at entry), flags | `Plugin { id, version, channels }` (+ the state hash until slice 4) |
| `Feedback { input, ring }` | the ring `Buf`, swapped | `input` | `Feedback { channels }` |
| `Audio(AudioNode)` | the tap state (`taps`, `ntaps`, `fade_in`, `prev_moving`, `cont`), copied | `lane: Arc<AudioLane>` (its clip renders, from the cache) | `Audio { lane: LaneAddr, channels }`, and its clips the same `Arc`s |
| lanes table, notes | `NoteScheduler`, moved by lane slot | its data `Arc<NotesData>` (swapped in at entry when it changed) | `LaneAddr` + kind; room checked |

These hold nothing that outlives a plan: `Const`, `Time`, `Control`, `Notes`, `Missing`, `NoteField`,
`Capture` and the new `Fade`. Control lanes are `Arc<ControlLane>` with no state, so the new plan just
holds its own `Arc`.

Why these boundaries:

- **A `poly` moves whole.** Its voice graphs, kernels, notes and buffers are one box. Kept or replaced
  is decided once, by `Shape::Poly`, which compares the voice graph's structure with constant values,
  origins and capture targets blanked. The p50 voice pools are 97% of all instances, so moving each as
  one pointer keeps prepare and entry O(top-level nodes), not O(instances).
- **The cost of moving a poly whole:** an edit inside an instrument's own voice graph replaces that
  instrument. Its notes end, with a 5 ms ring-out as a ghost. That is what happens today on every edit,
  and the contract's own simplification proposal 11 accepts it. Voice-by-voice matching can come with
  mono and legato.
- **A plugin's per-node lists depend on the caps of the lane it reads,** so they stay plan-side and are
  copied in at entry within their new room. This is today's `carry_from` copy, minus the adoption.
- **Audio data lives in the node, not in the lanes table.** A ghost audio node can then keep reading the
  old clip renders while its replacement reads the new ones. With the data in the table, both would
  read whatever the table holds. `SwapLane` never handled audio lanes anyway.

### 1.3 Identity: a continuation, not an id

- **The match uses step 6's `GraphDelta` names** (`core/session/src/tx/delta.rs`). A name is the module
  key, the frame chain, the op and its call place, then `#occurrence`. Every point in it is in the
  comparison's reference coordinates.
- **Where it is computed:** off the audio thread, in the session, for the pair the engine cares about.
  That pair is what plays (`Plays.revision`) and what is accepted, which is step 6's R4 basis.
- **What it produces:** `cont: Vec<Option<u32>>`, which maps each new top-level `Flat` node to the old one
  it continues.
- **What it is never:** stored, numbered or sent. The audio thread sees only `(old node, new node)` index
  pairs.

The contract's derived ids become:

- **SiteId** = (file, call place) as a point in the comparison's coordinates. Through slice 2 the call
  place is the call's first and last byte, as built. From slice 3 it is the point of its `(`.
- **InstancePath** = the name's module key and frame chain. Each frame is `key:<k>` or
  `<ComponentId>@<file>:<point>#<occurrence>`. From slice 3, nothing above the innermost `key()` frame is
  part of it.
- **RuntimeId** = the full name of a kernel, plugin, poly, feedback or audio-lane node.
- **ControlId** = the name of a declared control's constant (already built, `@<slot>`).
- **CellId** = an index in the active plan's cell table, unchanged.
- **A voice's RuntimeId** is its slot in its kept pool (RENDER-16).

---

## 2. `Plan` split from `DataSet`

### 2.1 Today

`Renderer` (`core/runtime/src/lib.rs`) is everything: one `Store`, the `Graph` (exec, buffers, versions),
`Cells`, `lanes: Vec<LaneSlot>` and the lane slots, the tempo map, the arrangement, the transport and the
clock. `DataSet` (`core/song/src/data.rs`) exists already.

### 2.2 After step 7

```rust
/// What an engine plays: one plan, the data its nodes read, and the song position.
pub struct Renderer {
    plan: Plan,                       // structure, from the Flat (and the instances it holds)
    lanes: Lanes,                     // from the DataSet: notes schedulers (state) and control lanes
    tempo: TempoMap, arrangement: Arc<Arrangement>,
    transport: Transport, clock: u64, next_note: u32,   // engine-wide note ids (§4.8)
    frames, runs, pending, delivered, strict, non_finite, broken, trace, dropped,   // as today
}

pub(crate) struct Plan {
    graph: Graph,                     // exec wrappers + bufs + versions; order_fading when ghosts exist
    order_after: Vec<usize>,          // the order once the fade window ends (ghosts left out)
    cells: Cells,                     // as today: per plan (RENDER-23)
    out: PortRef,                     // may point at a Fade node
    entry: Option<Entry>,             // what take_over does; None for a fresh plan
    ghosts: Range<u32>, fade: u32, faded_from: Option<u64>, leftovers: Vec<Exec>,   // §4.3 step 8
    info: Arc<PlanInfo>,
}

pub(crate) struct Lanes {
    slots: Vec<LaneSlot>,             // LaneSlot::Notes(Option<NoteScheduler>) | LaneSlot::Control(Arc<ControlLane>)
    refs: Vec<LaneRef>,               // module, lane, kind: what lane_slots() gives
}
```

What is built from what:

- **`Plan` from the `Flat` and the packages only:** `Builder::graph`, the per-node exec wrappers, fresh
  instances, `cells(flat)`, and the ghost and fade nodes.
- **`Lanes` from the `DataSet` and the tempo map:** `NotesData::build`, `data.control()`.
- **Audio clip renders from the `DataSet` through `audio::Cache`:** an `Arc<AudioLane>` per audio node.

A change of data alone never builds a `Plan`: `SwapLane` replaces lane data in the running `Lanes`, as
today. The contract's `DataSet { arrangement, lanes }` is `m2_song::DataSet`, unchanged.

**Why keep the name `Renderer`.** It is the object `Engine`, `Player`, `m2_editor::render::Render`, the
CLI and about 40 tests already use. Under it, the contract's split is visible as `plan` and `lanes`.
Renaming it would buy nothing.

### 2.3 The constructors

```rust
impl Renderer {
    /// Offline, CLI, tests: every instance new. Unchanged signature.
    pub fn new(flat: &Flat, data: &DataSet, packages: &PackageSet, rate: u32) -> Result<Renderer>;
    /// Everything else. Replaces build_with and build_live.
    pub fn prepare(flat: &Flat, data: &DataSet, packages: &PackageSet, rate: u32,
                   cache: &mut audio::Cache, owner: &Owner, basis: Basis) -> Result<Renderer>;
    pub fn info(&self) -> Arc<PlanInfo>;
}

pub enum Basis<'a> {
    /// Every instance new: an offline render, a new engine (`Player::open`, a rate change, a new player).
    Fresh,
    /// Move whatever continues from the plan the engine will be playing when this one arrives.
    Keep { flat: &'a Flat, info: &'a PlanInfo, cont: &'a [Option<u32>], restarts: &'a HashSet<Handle> },
    /// An offline render that shares live plugin instances (export, RENDER-1); kernels are fresh.
    Borrow { plugins: &'a HashMap<usize, Arc<PluginInstance>> },
}
```

`Renderer::starting_at` (range render) and `seek` (offline replay) are unchanged.

---

## 3. Identity

### 3.1 `continues` (slice 2, on the names as built)

There is one new function, in `core/session/src/tx/continues.rs`:

```rust
pub struct Continues { pub nodes: Vec<Option<u32>> }   // new top-level Flat node -> old one
pub fn continues(old: &Revision, new: &Revision) -> (GraphDelta, Continues);
```

How it works:

1. It builds the same `Coords::revisions(Edits::between(..))` per changed code file as `delta_between`,
   and calls `delta::compare`, which already returns both sides' `Graph`s.
2. `Side::name_all` records `ids: HashMap<(module, ir node id), Name>` in each `Graph`, about 10 lines.
3. For each top-level `FNode` of the new `Flat`, it splits `FNode.name` (`<module>/<node id>`), looks up
   its name, looks for the same name on the old side, then finds the old `FNode` index by name.
   `FNode.name` is the join key, and it holds for every top-level node.
4. Nodes inside voices aren't matched: a poly moves whole.

The actor's `reach` calls `delta_between` on every settle today. It calls `continues` instead and reuses
the delta, so the extra cost is the join: three hash maps over about 2,500 top-level nodes at p50
(*estimate*).

Why this is enough for the gate:

- Untouched tracks are in unchanged files. Their names are `Base` points on both sides.
- Package (std) sites are always `Base`.
- So every node of an untouched track, and the mixer in an untouched `song.ts`, continues. Only the
  edited file's names can change.
- In slice 2, an edit renames the calls whose first or last byte falls in its line hunk. For an effect
  wrapped around an instrument, that is the instrument itself (identity notes F2). So in slice 2 the
  wrapped track behaves as it does today: its instrument restarts. Every other track keeps everything.

### 3.2 Names that survive edits (slice 3)

These are the four defects from `understand-identity.md`. All four are on the engine's path
(`delta_between`), so every writer gets the fix, agents included (PLANE-1):

| Fix | Where | Size |
|---|---|---|
| **F1** Refine each line hunk with a token-level Myers diff. It reuses the generic `kept_lines` over identifier, number, string and single-punctuation tokens; past `DIFF_LINES` tokens it keeps today's hunk | `core/session/src/text.rs` `Edits::between` | about 80 lines |
| **F2** Name a call by the point of its `(`: a new `Site.open`, found by scanning from the callee's span end past whitespace, comments, `?.` and type arguments | `core/script/src/sites.rs` `visit_call_expression`; `delta.rs` `Side::place`/`spell` | about 15 + 20 lines |
| **F3** Put the ComponentId in each chain segment: `<ComponentId>@<file>:<point>#<occ>`, so a component swap at one `(` is new and an alias (`delay as echo`) is not | `delta.rs` `Side::chain` | about 5 lines |
| **F6** `key()` pins: drop every segment above the innermost `key:` frame (keys are unique per parent key frame) | `delta.rs` `Side::chain` | about 10 lines |
| **F5 / ID-14** `Facts.counts`: for each chain segment with an occurrence, how many frames share its (parent, site), then the size of its own `#n` group. `continues` keeps a pair only when the counts are equal | `delta.rs` `chain`, `name_all`; `continues.rs` | about 25 lines |
| **F7** Keyed plugins, second pass: an unmatched new plugin with `state: Some(path)` takes the unmatched old plugin of the same module, path, id and version. It is unambiguous by `plugin.state-collision` | `continues.rs` | about 20 lines |

`places`, which judge `Confined`, still list every frame's call range, so expectations judge exactly as
before. The delta's hash spelling changes once. Hashes are computed per check and compared within one
version, and step 6's two-process gate still holds because the names stay deterministic (risk R7).

Also in slice 3, both optional and off the critical path:

- **The editor's text carry.** `crates/editor/src/controls.rs` `carry_text`, `carry_ends` and
  `carry_lines` become `m2_session::text::Edits::between(old, new).carry(range)`, so leases, cables and
  undo carry by one correspondence (WRITE-12, WRITE-22).
- **`m2_ir::control_id`.** Only a test uses it, so it is deleted.

### 3.3 The rewired note consumer (F8)

A kept poly or plugin whose notes lane changed (`lane.notes("a")` to `"b"`) holds notes the new lane's
scheduler doesn't know, and they would stick. `prepare` compares the old and new `LaneAddr` of each kept
note consumer. When they differ, the new node is listed in `Entry.release`, and its notes end at entry,
exactly as a ghost's do (§4.6). No delta field is needed: it is a comparison of two `FOp`s.

---

## 4. The arena

### 4.1 Shape

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Kernel   { id: String, wasm: Sha, channels: u8 },
    Plugin   { id: String, version: String, channels: u8, state: Option<Sha> },  // `state` until slice 4
    Poly     { voices: u16, channels: u8, voice: Arc<Voice>, wasm: Vec<Sha> },     // voice: blanked (below)
    Feedback { channels: u8 },
    Audio    { lane: LaneAddr, channels: u8, clips: Vec<(u64, usize)> },           // clip starts and Arc pointers
}
```

- **`wasm`** is the sha-256 of the package's wasm module, from `Package.files`
  (`PackageSet::module_sha(kernel_id)`, about 8 new lines). It is a content hash, so two `PackageSet`s
  loaded from the same bytes give equal shapes even though their wasmtime `Module`s differ. std is one
  module for all its kernels, so rebuilding `std.wasm` changes the shape of every std instance. That is
  safe, and it is what G7d checks (RENDER-25 edit: the kernel's module hash is its package module's).
- **A blanked `Voice`** is the voice `Flat` with every `FOp::Const` value, every origin, `src` and name,
  and every `Capture` target cleared, and capture channel counts kept. This is the server's `bare()`
  from `value_writes`, applied to one voice. `Input::Const` values stay in it: a plain number edited
  inside an instrument body replaces the instrument. That is rare, since std's controls are cells.
- **An audio node keeps its taps** only when its clip renders are the same `Arc`s. A cache hit returns
  the same `Arc`. A tempo change re-renders every clip, so every audio node is replaced and crossfaded
  (§5.1).
- **A plugin that asked for a restart** (`HostEvent::Restart`) has a mismatched shape at the next
  activation. The actor keeps `restarts: HashSet<Handle>`, which replaces `Carry::restart` and
  `PluginInstance.restart`.

### 4.2 `PlanInfo`: what the next prepare reads

```rust
pub struct PlanInfo {
    pub generation: u64,
    pub rate: u32,
    pub shapes: Vec<Option<Shape>>,                    // by top-level Flat node; None: holds nothing (or unreached)
    pub notes: Vec<(LaneAddr, Caps, Arc<NotesData>)>,  // by lane slot
    pub control: Vec<(LaneAddr, Arc<ControlLane>)>,
    pub audio: HashMap<usize, Arc<AudioLane>>,         // by node: for the equality check and a ghost
    pub plugins: Vec<(usize, Arc<PluginInstance>, Option<Vec<String>>)>,  // node, instance, state path
    pub index: HashMap<String, usize>,                 // FNode.name -> node: gates' taps, the export's borrow
    pub cell_slots: Arc<CellSlots>, pub cell_map: Arc<CellMap>,
}
```

- **Where it comes from:** `prepare` writes it, and the server keeps it in `Plays.info`. It replaces
  `Plays.{slots, caps, cell_slots, cells}` and `Open.carry`.
- **It is immutable.** The next prepare reads only `PlanInfo` and the old `Flat` (`Plays.revision`), and
  never the live plan, which belongs to the audio thread.
- **Why `Plays.revision`'s `Flat` is the right old `Flat`:** `adopt` moves `Plays.revision` forward only
  when the graphs are equal but for values (`value_writes`), so its node indices are the playing plan's.

The `Arc<PluginInstance>` clones keep an instance alive until the `PlanInfo` that names it is replaced.
That happens at the next settle, on the actor thread, where its destruction is queued to the plugin main
thread as today.

### 4.3 `prepare` with `Basis::Keep`, step by step (off the audio thread)

Inputs: the old `Flat` O, its `PlanInfo` I, the new `Flat` N, `cont`, `restarts`.

1. **Lanes.** For each lane `lane_slots(N)` gives:
   - **A notes lane in I with equal kind:** a `LaneMove { from, to, data, grow }`.
     - `data` is the new `Arc<NotesData>` when the lane's data or the tempo changed, else `None`.
     - `grow` holds a fresh `NoteScheduler` sized for the new data when `needs` exceeds the old caps.
   - **Any other notes lane:** a fresh scheduler.
   - **A control lane:** its `Arc`, taken from I when unchanged, else built.
2. **Matching stateful nodes.** For each node `i` in `N.order` that holds an instance, it is kept when
   all of these hold:
   - `cont[i] == Some(j)`;
   - `I.shapes[j] == Some(shape(N, i))`;
   - `j` isn't claimed already;
   - `j`'s plugin handle isn't in `restarts`.

   A kept node is claimed and goes on `moves` as `(j, i)`. Everything else gets a fresh instance:
   - kernels: `Store::new` and `Instance::new`, typed funcs, `k_init`, the port checks and the prewarm
     reset, as today, one store each;
   - plugins: `host.create`, then `activate` on the plugin main thread while the old plan plays;
   - polys: every voice graph built;
   - feedback rings and audio nodes: new.

   Pure nodes (`Const`, `Time`, `Lane`, `Missing`) need no instance. Each one *continues* when
   `cont[i] == Some(j)` and the op kind is the same. `same(i)` is `j` for a kept or continuing node, else
   `None`. `new_of[j] = i` is its inverse.
3. **Edges.** For each kept node `i` (and each continuing `Missing`), compare each input `p` with old
   node `j`'s input `p`. The inputs are positionally equal because the shapes are equal. The inputs
   compared are:
   - a kernel's inputs;
   - a plugin's `in`;
   - a feedback's `input`;
   - a poly's captures, which are equal in number because the voice graph is.

   Two inputs are the same when:
   - both are ports with `same(new src) == Some(old src)` and the same port index; or
   - both are `Const` with equal bits; or
   - both are `Unset` (plugins only; kernel inputs always resolve to `Const` or a port).

   Anything else is a **faded edge** `(i, p, old, new)`. The song output is checked the same way:
   `N.out` against `O.out`.
4. **Ghosts.** The worklist starts with every faded edge's old source whose `new_of` is `None`. Each old
   node `j` taken from the worklist:
   - gets a ghost index;
   - pushes each of its port inputs (feedback and captures included) whose `new_of` is `None`.

   The closure stops at old nodes that continue. Each ghost is appended to the new plan as a clone of
   `O[j]`, with each input remapped to `new_of[src]` or to that source's ghost index.
   - **Stateful ghosts:** a move `(j, ghost)`. A ghost poly or ghost plugin is flagged `ghost`: it is
     released at entry and reads no events.
   - **Pure ghosts:**
     - a `Const` keeps its old value;
     - `Time` is `Time`;
     - a control `Lane` whose `LaneAddr` is still in the new lanes table reads its slot, and one whose
       lane is gone outputs its default;
     - an audio lane ghost gets `I.audio[j]` and is flagged free-running (§5.1).
5. **Fade nodes.** For each faded edge, append `Exec::Fade { old, new, channels }`:
   - `old` is the remapped old input, or `Const(0.0)` when that input was `Unset`;
   - `new` is the new input;
   - `channels` is the new source's channel count; a mono side is duplicated, as kernel input copying
     does.

   The consumer's input `p` (or `out`) is pointed at the fade node.
6. **Order.**
   - No ghosts or fades: `N.order`, unchanged.
   - Otherwise: Kahn's algorithm, lowest index first, over N plus ghosts plus fades, with feedback inputs
     not counted, which is check.rs's rule, in about 40 lines. `order_after` is that order without the
     ghosts. The fade nodes stay in it as exact copies until the next plan drops them.
7. **Cells.**
   - `cells(N)`, `CellMap::of` and `CellSlots::new`, as today.
   - `Entry.resync` lists every voice cell (`CellAt::Voice`) of a kept poly. Those constants live in the
     moved voice graphs, so they are written at entry, and only when they differ (§4.5).
   - The server then sets each kept lease's value with `Renderer::set_cells`, as today. For a kept
     poly's voice cell, this sets only `Cell.value`, since the pool arrives at entry.
8. **The entry.** It is all sized here and never grows. The one-shot lists are in `Entry`, which leaves
   the thread with the old renderer once applied. What the plan needs after entry lives on `Plan`:

   ```rust
   pub(crate) struct Entry {
       after: u64,                 // the generation it was prepared against
       moves: Vec<(u32, u32)>,     // (old node, new node), kinds agree; ghosts included
       lanes: Vec<LaneMove>,       // { from, to, data: Option<Arc<NotesData>>, grow: Option<NoteScheduler> }
       resync: Vec<CellId>,
       release: Vec<u32>,          // new nodes whose notes end at entry: ghosts, rewired (F8)
       pub stats: Stats,           // kept / fresh / ghost / fade counts by kind (gates)
   }
   // and on Plan, used after entry:
   //   ghosts: Range<u32>         node indices of the ghosts
   //   fade: u32                  round(0.005 × rate) frames; 0 when nothing fades
   //   faded_from: Option<u64>    the engine clock of entry, set by take_over
   //   leftovers: Vec<Exec>       capacity = ghosts.len(): where ghosts go at the fade's end
   ```

`prepare` never touches a live instance. A prepare that fails (a plugin that won't load) has no effect
on what plays: its fresh instances drop on the actor thread, and the old plan plays on.

### 4.4 The audio-thread handoff

The message is unchanged: `Msg::Activate { renderer: Box<Renderer> }`, the new renderer carrying its
`Entry`. `Player::activate` is unchanged.

`Engine::try_apply(Activate)` (`core/runtime/src/engine.rs`):

1. **Ready** when the playing plan has no fade running (`!current.fading()`) and the stash has room.
   Otherwise it waits at the head, as today, for at most 2 blocks. Activations are never coalesced: each
   entry names the plan it was prepared against.
2. `renderer.take_over(&mut old)`, below. If it refuses, the new renderer is retired unplayed,
   `refused_activations` is counted, and a `debug_assert` fires. It refuses only when `entry.after`
   isn't `old.generation()`, which needs a dropped `Activate`; the player reports that as `Stalled`, and
   the actor reloads.
3. `old` is retired whole, `Retired::Renderer(old)`. It takes with it every instance nothing moved, its
   plan buffers, and the new renderer's spent `Entry` (step 8 below). `current = renderer`.

`Renderer::take_over(&mut self, old: &mut Renderer) -> bool` does, in order, with no allocation:

1. Check `after == old.generation()`.
2. **Moves.** For each `(from, to)`, `self.graph.exec[to].take_from(&mut old.graph.exec[from])`:
   - `Kernel`: `inst = old.inst.take()`.
   - `Poly`: `pool = old.pool.take()`.
   - `Plugin`:
     - `inst` is moved;
     - `keys`, `ended` and `status` are copied within their room;
     - for a node flagged `ghost` or `release`, the keys go to `release` instead, to get a NoteOff at its
       first call.
   - `Feedback`: `mem::swap` of the rings; the prepared ring goes back with the old plan.
   - `Audio`: copy of the tap fields.

   A pair whose kinds disagree fires a `debug_assert` and is skipped: the new node plays silence, which
   prepare's own check makes unreachable.
3. **Lanes.** For each `LaneMove`:
   - **Plain move:** `self.lanes.slots[to] = old.lanes.slots[from].take()`.
   - **With `data`:** `swap(data)`. The replaced `Arc<NotesData>` is put back into the `LaneMove`'s
     `data` field, so it leaves with the spent `Entry` (step 8), never dropped here.
   - **With `grow`:** the fresh scheduler `take_state(&mut moved)`. That copies `sounding` (it fits: it
     is bigger) and sets `swapped`. The small one goes back into `old.lanes.slots[from]`.
4. **Song position.**
   - `self.transport = old.transport`, then `retime(old.tempo → self.tempo)` and `enter()` (Jump::Enter).
   - `self.clock = old.clock` and `self.next_note = old.next_note`.
5. **Cells.** For each `resync` cell, if the moved voice graph's constant isn't the table's value (bits),
   write it as a step. This is O(voice cells of kept polys), compares only, about 1,500 at p50
   (*estimate*).
6. **Release.** For each `release` node:
   - a poly: every voice with a note gets `note.off = min(off, clock)`;
   - a plugin: done in step 2.
7. Set `faded_from = Some(clock)` when the plan has fades.
8. `mem::swap(&mut self.plan.entry, &mut old.plan.entry)`. The spent entry, with the replaced lane data
   in it, now leaves the thread inside `old`.

Audio nodes: `AudioNode::process`'s `Jump::Enter` arm stops clearing `fade_in`. A fresh node has none,
and a kept one goes on with its own state, so a fade-in in progress isn't cut and a no-op activation
stays bit-exact.

**At a block line during the fade** (`Engine::block`, before messages): when `clock ≥ faded_from + fade` and
the stash has room, `current.end_fade()` runs:

- each ghost's `Exec` is swapped with `Exec::Const(0.0)` into `leftovers`, which was pre-sized;
- `graph.order` becomes `order_after`;
- `Retired::Nodes(leftovers)` leaves the thread.

Without stash room it waits a block: the ghosts play on, faded to nothing.

**What never reaches the audio thread:** names, hashes, `Flat`s, the `PlanInfo` (an `Arc` the renderer
holds but never reads there), and any `Vec` that must grow.

### 4.5 Cells across plans

Cells stay per plan (RENDER-23 as is). The rest follows:

- **Top-level constants** are new each plan and are filled at prepare with the new `Flat`'s values plus
  each kept lease's value.
- **A kept kernel's wrapper** starts with `seen = COPY` on every input, so its first call in the new plan
  copies every port input once.
- **`consts_written = false`** makes it write every `Input::Const` into its memory once. The values are
  the same unless the edit changed them.
- **Leases.** The server re-aims leases at the new plan's slots under the control lock in the same phase
  that sends the `Activate`, as today. An update before the send is in the value `set_cells` used, and an
  update after it goes to the new slots, which the new plan drains after `take_over`. So `resync` never
  overwrites a newer value with an older one.
- **The fast path stays.** A value edit whose graph numbers its cells alike still takes `SetCell` with
  no activation (`same_layout` and `value_writes`, unchanged). Any other value edit is an activation
  that keeps every instance, so the fallback that cut every tail today now cuts none.

### 4.6 Fades, exactly

`Exec::Fade { old: Input, new: Input, channels: u8 }`. For each channel and frame, with `k` the engine
frames since entry and `len = fade`:

```
y = if k + 1 >= len               { n }          // after the window: an exact copy of the new source
    else if o.to_bits() == n.to_bits() { o }      // identical signals stay bit-identical, -0.0 included
    else                           { o + (n - o) * ((k + 1) as f32 / len as f32) }   // linear: correlated material
```

- **Linear** is what the whole-output fade uses today (correlated material: dry against wet, the same
  instrument).
- **The bit-equality branch** makes a silent structural edit bit-exact through the window. That is what
  lets G7b demand bit identity rather than "within an ulp".
- **After the window** the fade node is an exact copy (its output's version is `COPY`) until the next
  plan drops it.

Where fades fall:

- **An inserted effect:** `pluck → gain` becomes `pluck → delay → gain`. `gain.in` fades from `pluck.out`
  (kept) to `delay.out` (new), dry to wet, and needs no ghost.
- **A removed effect:** `pluck → delay → gain` becomes `pluck → gain`. `gain.in` fades from the delay
  (a ghost, still reading the kept pluck) to `pluck.out`, wet to dry.
- **A replaced instrument:** `polyA → gain` becomes `polyB → gain`. `gain.in` fades from ghost A,
  released at entry so it rings out for 5 ms, to the fresh B.
- **The master replaced:** the song-output edge fades from the old chain (ghosts) to the new.
- **A package rebuilt with every std kernel new (G7d):** every kernel is fresh. The ghost closure is the
  whole old graph, so 5 ms run at twice the DSP, which is exactly today's cost on every edit. It happens
  only on a package change.

**Ghost note consumers:**

- A ghost poly reads an empty event slice.
- A ghost plugin gets no lane events.
- Their notes end at entry (RENDER-26a's "replaces a note consumer"), and their releases ring for the
  window.
- A ghost audio node free-runs: it reads its old renders from `cont` onward and ignores the runs, so a
  retimed transport can't make it jump (§5.1).

**Jumps and panic during the window:** jumps apply to ghosts as to every node. Panic resets everything
in the plan, ghosts included, and the window ends on time. The engine doesn't idle while a fade runs.

### 4.7 Message order (RENDER-28)

- Structural messages apply in strict order, as today.
- The coalescing branch of `Engine::block` (a newer `Activate` replacing one that never played) is
  deleted: a move list is relative to the plan it was prepared against.
- The actor stays serial. It prepares each activation against the plan it sent last, which the engine
  will be playing by the time it reads this one. So no acknowledgement is waited for.
- `SwapLane { generation, slot, data }` keeps its address. The actor sends it to the generation it sent
  last, which strict order makes the current one when it is read. The generation check becomes a
  `debug_assert`.
- `Player::send` still reports `Stalled` after 500 ms and drops the message. The actor's next settle
  then uses `Way::Load`, a new player and fresh instances, as today. The contract's RENDER-28 text is
  updated to say so.

### 4.8 Schedulers and note ids

- **One scheduler per lane slot moves from plan to plan,** so a kept consumer keeps receiving its lane's
  Offs. This deletes `NoteScheduler::adopt`, `adopted`, `released`, `release_all` and `run`'s
  `release_all` argument.
- **Jump::Enter** still seeks a cursor without ending notes. For a moved scheduler that changes nothing;
  for a fresh one it is today's start.
- **Note ids are engine-wide:** `Renderer.next_note: u32`, passed as `&mut` to every `scheduler.run`,
  copied at entry, and starting at 1 offline.
  - **What this deletes:** `NoteScheduler.next_id`, `next_id()`, `raise_next_id`,
    `PluginInstance.next_id` and `held_cap`.
  - **Why it is enough:** an id is never repeated into any plugin, whatever lane it reads.
  - **Why it changes no bit:** ids reach no kernel. A `poly` matches Offs by id and allocates voices by
    `born`, so offline renders of the pure fixtures stay byte-identical, and slice 1 checks that.
- **A note consumer replaced, or kept and rewired to another lane:** its notes end at entry (§4.4
  step 6).
- **A new consumer of a kept scheduler** gets Offs for notes it never got Ons for. A `poly` and
  `plugin_events` ignore those, as they ignore a stolen note's Off today.

### 4.9 What an activation costs at p50 (*estimates*)

| Part | Today | After |
|---|---|---|
| Kernel instances built | about 6,100 | the new ones: 1 to 3 for an added effect, about 156 for a replaced pluck |
| Plan-side wrappers and buffers | inside the build | about 2,500 top-level nodes, about 2.5 MB zeroed: 1 to 3 ms |
| `continues` (delta plus join) | the delta alone | plus about 1 ms |
| Audio-thread entry | `enter_from`, `carry_from` | about 300 moves (kernels, polys, rings), about 100 lanes, about 1,500 cell compares: tens of µs |
| CPU during the 5 ms | twice the whole song | the song plus the ghost closure: a few nodes |
| Teardown on `m2-play-drop` | about 6,100 instances unmapped | the replaced ones plus plan buffers |

---

## 5. Arrangement switches and prepared data

### 5.1 Tempo and meter (RENDER-27): an activation of the same structure

`reach` already refuses arrangement changes, so they come to `way`, which prepares with `Basis::Keep`.

**What is kept and what is rebuilt:**

- Every kernel, poly, plugin and feedback continues: same names, same shapes.
- Every notes lane gets a `LaneMove` with new `NotesData`, built in the new tempo map.
- Every control lane gets its `Arc` again. Control lanes are by beat, so they continue.
- Every audio lane's clips are re-rendered through the cache, because the tempo is in every clip's key,
  so every audio node is replaced.

**What happens at the block line:**

- **All of it switches together** (RENDER-27): the moves, the lane data and the new `TempoMap` and
  `Arrangement` in the new renderer.
- **The transport keeps its beat.** `take_over` copies the old transport and retimes it into the new
  map, as `enter_from` does today.
- **Pending Offs are recomputed from their beats.** At its first block, each moved scheduler with new
  data reconciles its sounding notes against the new data by `(clip, pitch, start beat)` and takes each
  note's new `end` and `end_beat` (`sched.rs` `reconcile`). That is RENDER-26a's third bullet, with no
  new code.

**The audio lanes crossfade.** Each audio node's consumer edge fades from the ghost, which is the old
render free-running from its own continuation, to the new node, which is the new render at the retimed
position. The free-running ghost is about 5 lines in `AudioNode` (`process_free`).

### 5.2 Prepared data: audio lanes, assets, notes lanes that outgrow their room

- **An audio-lane edit or a changed asset** is not a `SwapLane`, which never handled audio lanes. It is
  an activation of the same structure:
  - the edited lane's clips are rendered on the actor thread through the cache, while the old plan plays
    the old clips (RENDER-27, last sentence);
  - its audio node's clips differ, so the node is replaced and crossfaded at its consumer;
  - every other instance moves.
- **A notes edit whose data needs more room than the scheduler has:** today that is a rebuild. Now it is
  an activation of the same structure with a `grow` lane move, and the consumers' event lists are sized
  for the new caps in the new wrappers. Nothing restarts.
- **Notes and control lane edits that fit** stay `SwapLane`, unchanged: the 20 ms plane.

### 5.3 Planes table, as built

| Plane | Path after step 7 |
|---|---|
| Data (notes, control) | `SwapLane`, as today |
| Prepared data: audio lanes, assets | renders prepared off the audio thread; `Activate` of the same structure (the audio node crossfades) |
| Prepared data: plugin state | `PluginHost::load_state` on the plugin main thread (§6.2) |
| Arrangement | tables and renders prepared off the audio thread; `Activate` of the same structure |
| Code | compile, check, verify; `continues`; prepare; `Activate` |

---

## 6. Plugins

### 6.1 Persistent instances

Plugin instances move like every other box (§4.4), by name and `Shape::Plugin`. This replaces
`CarryKey`, whose ordinal matching hands an old instance's state to a newly inserted node (identity
notes, §6.2 of the runtime notes). The changes:

- **Keyed plugins** match by state path in slice 3's second pass (F7).
- **Plugins inside `poly`** are refused by the checker (`check.rs`, "plugins can't be inside poly"), so
  the runtime's `in_voice` branch is deleted.
- **A changed state blob:** in slice 2 the shape still includes the state hash, so the instance is new,
  exactly as today. Slice 4 removes the hash from the shape and loads the state instead.

### 6.2 `LoadState` (slice 4): a main-thread call, not an engine message

When the delta lists a state change (`GraphDelta.states`, by `module/frames/key`) and the plugin that
uses that state path continues, the actor works through `PlanInfo.plugins`, by state path:

1. Read the blob the snapshot verified (`DataSet::state`).
2. `host.load_state(h, blob)` on the plugin main thread. The host skips it when its hash is the
   instance's last save (HOME-6). CLAP's `state.load` is a main-thread call allowed while the plugin is
   active.
3. `inst.reloaded.store(true)`, a new `AtomicBool` on `PluginInstance`. At its next call the node swaps
   it, clears every parameter's last-sent value so that code-owned values are sent again (HOME-4), and
   clears `keys` (the plugin's voices are its own after a load).
4. `host.latency(h)`. If the latency is nonzero, a Warning `plugin.latency-uncompensated` names the
   plugin. Compensation doesn't exist and the check refuses latency at default state (RENDER-21), so
   there is nothing to re-plan. `Plan.latency` waits for compensation.

The rest of the path:

- **A state-only change** never builds a plan. `reach` gains a `states` list next to its cell writes and
  lane swaps. It is loaded in place, and `Plays.revision` is adopted as after a lane swap; the state
  hash isn't in the `Flat`, so the graphs are equal.
- **Mixed revisions:** when a state change rides with a structural or arrangement change, the loads run
  after `prepare` succeeds and before the `Activate` is sent.
- **A state blob removed:** there is no "load defaults" in CLAP, so the shape mismatches and the
  instance is new.
- **Host fix,** one line in `core/plugins/src/inprocess.rs` `load_state`: after a successful load, set
  `last_saved = Some(blob.sha)`. "The instance holds this blob now." Without it, a save then a load of
  another blob then a load of the saved one would skip the last load. Export's restore depends on this.

### 6.3 Export: pause, borrow, run, restore, resume (slice 4)

A song whose export plan borrows at least one live instance pauses playback. A pure song, or one whose
plugins are all missing, exports beside playback as today.

**On the actor** (`Actor::export`, which replaces `Actor::render` for Export Audio):

1. `rev = accepted`. Then `continues(plays.revision, rev)`, and for each plugin node of `rev.flat` that
   continues with an equal shape, map it to its live `Arc<PluginInstance>` (`PlanInfo.plugins`).
2. `Render::borrowing(flat, data, packages, rate, Length::Song, &borrowed)` prepares with
   `Basis::Borrow`:
   - kernels are fresh, from init (RENDER-1);
   - borrowed plugin nodes are flagged `reset_first`;
   - other plugins are created and destroyed after.
3. `o.lent = true`. The engine view publishes `paused: "export"`.
4. `live = player.lend()?`. This is a new `Msg::Lend`:
   - the callback fades the `Live` out over 5 ms with today's `LiveFade`;
   - instead of retiring it, it sends it back on a `bounded(1)` return channel, then plays silence;
   - `lend` waits for it for up to `STALL`.

   While the `Live` is out nothing calls `process` on its instances, which the host requires before
   `deactivate` (`inprocess.rs` `stop`: "no renderer holds it").
5. Spawn `music2-export` with `(live, render, borrowed)`, and return.

**On the export thread:**

1. For each borrowed instance:
   - `saved = save_state(h)`;
   - `deactivate(h)`;
   - `load_state(h, committed blob)`, when the revision has one;
   - `set_render_mode(h, Offline)`;
   - `activate(h, rate, BLOCK)`.
2. Render. At a `reset_first` node's first call the export thread calls `host.reset(h)`: CLAP's `reset`
   needs an active instance (`audio.rs` `reset`: "Only while active"), so it comes after `activate`
   (RENDER-1 order edit). Code-owned parameters are sent at the first call, because the export plan's
   last-sent values are `None`.
3. For each borrowed instance:
   - `deactivate(h)`;
   - `set_render_mode(h, Realtime)`;
   - `load_state(h, saved)`;
   - `activate(h, rate, BLOCK)`;
   - `inst.reloaded = true`, so the live node sends its code-owned parameters again.
4. Drop the export renderer: its `Arc`s go, and the instances live on in the live plan.
5. `player.give_back(live)`. This is `Msg::Replace { live, keep: Keep::Nothing }` with a new 5 ms device
   fade-in for a `Live` that resumes mid-song, a sibling of `LiveFade` in `Callback`, about 15 lines.
6. Post `ExportDone` to the actor: `o.lent = false`, then `settle()`.

A guard object does steps 3 to 6 on every path out, errors and panics included. Stopping the export
(the app is quitting) is the same path.

**Meanwhile:**

- **Kernel state is frozen:** the engine doesn't run. The transport resumes where it was, playing if it
  was.
- **Plugins resume with the state they had** (the gate). Their voices and tails were reset, which is
  RENDER-1's stated cost.
- **The actor still takes revisions** (edits, saves, the watcher) but sends nothing to the engine while
  `lent` is set. `settle` publishes the view and skips `way`. When the `Live` comes back, `ExportDone`'s
  settle compares what plays with what is accepted and activates once (step 6's R4 basis), so nothing is
  lost or reordered.
- **Transport routes** answer "Playback is paused while the song exports" while `lent` is set, and the
  web disables the transport. Messages that did arrive would be retired unapplied by a `Callback` with
  no `Live`, so they must not be sent.

The CLI (`m2 render`, `m2 bounce`) has no live engine, so `Renderer::new` creates its own instances, as
today. The range render for agents (RENDER-4) also creates its own, as today.

### 6.4 "loading <plugin>" (RENDER-25, slice 4)

While `prepare` creates plugin instances, the actor publishes `loading: [plugin names]` in the engine
view through the control plane, before each `create` and cleared after. The web shows "Loading Surge
XT…" in the status area. The old plan plays meanwhile, and the 200 ms gate doesn't count it.

---

## 7. The Store layout (RENDER-32)

### 7.1 The code shape

`KernelInst` owns `store: Store<StoreLimits>`, with:

- `memory_size(KERNEL_MEMORY)` and `instances(1)`, `memories(1)`, `tables(1)`;
- its `Memory` and `TypedFunc`s, which are valid in that store only.

`Graph::process`, `reset`, `prewarm`, `active` and `kernel_memory` lose their `store` argument. Each
kernel call goes through its own store: `k.process.call(&mut k.store, n)`.

Why one per instance, by elimination:

- **A shared store leaks.** wasmtime frees nothing in a `Store` until it drops (`store.rs`: "intended to
  be a short-lived object"), so one long-lived arena store would keep every replaced instance's memory.
- **A shared store can't move.** Its instances can't be handed to another plan.
- **Mixed package sets need separate stores.** Instances from two `PackageSet`s (two wasmtime `Engine`s,
  as in G7d) can only live side by side in separate stores.
- **The cost scales with memories, not stores.** Each memory reserves its own 4 GiB plus guards and
  copies its data segments whatever the layout. A store adds a `StoreOpaque`.

### 7.2 G7f, measured first (slice 1's first commit)

`m2-gates store`, in `crates/gates/src/store.rs` (new): engine level, no server, no device.

1. **Count.** Compile p50 (`lab.reference(&Reference::P50)`). The count is top-level kernel nodes plus,
   for each poly, voices times the kernel nodes in its voice graph. Print it next to the contract's
   6,176; the runtime notes count about 6,060. The contract's number is set from the printed one.
2. **For each layout,** build every instance as `Builder::kernel` does (instantiate, typed funcs,
   `k_init`, port checks, one prewarm reset):
   - **A:** one `Store` for all (HEAD's);
   - **B:** one per instance, on-demand allocator, default tunables;
   - **C:** one per instance, `memory_reservation(32 MiB + 64 KiB)` and `memory_guard_size(64 KiB)`,
     explicit bounds checks;
   - **D:** one per instance, `InstanceAllocationStrategy::Pooling` with `total_memories` and
     `total_core_instances` set to twice the count and `max_memory_size = 32 MiB`. This needs wasmtime's
     `pooling-allocator` feature in the workspace `Cargo.toml`, a measured option only.
3. **Measure, per layout:**
   - build time, total and per instance (µs);
   - `task_info` `phys_footprint` and `virtual_size` before and after;
   - per-block time to run `k_process(128)` on every instance in plan order, with inputs copied, over
     11,250 blocks (30 s): mean, p99, max;
   - time to build 1, 14 and 156 instances (an effect, a bass voice pool, a pluck pool);
   - time to drop all of them, and the longest single drop;
   - the largest count at which instantiation still succeeds, up to 4× p50, which catches an
     address-space ceiling.
4. **Record** in the gate's report and in RENDER-32's Today line.

**The decision rule, stated before measuring:**

- Choose **B** unless one of these fails against A:
  - p99 per-block time is worse by more than 80 µs (3% of the 2.67 ms block);
  - footprint is worse by more than 64 MB;
  - instantiation fails below 2× p50;
  - p50's full build is worse by more than 50%.
- If B fails, choose **D** if it passes, then **C** if it passes on everything but the bounds-check cost,
  measured as p99.
- A, and per-batch stores (several instances sharing one store, dropped with the last of them), are not
  options: A leaks and can't move, and per-batch stores need a store table re-indexed on every
  activation, which this design doesn't build. If B, C and D all fail, step 7 stops and the owner
  decides.

After slice 2, G7f's second half runs the real arena: p50 at 3:00, 30 s, p99 callback time with the
chosen layout against slice 1's A numbers, and recorded.

---

## 8. The gates, and how each is measured

Every gate runs in `crates/gates`. Timed ones run in a recorded release run on the reference Mac at
48 kHz with 512-frame device buffers. The rest run in CI.

### G7a. "In p50, a structural edit at 3:00 is audible within 200 ms after the compile"

- **Server gate:** `m2-gates --server --reference 50 --at 180 add-effect`.
  - `--at` is a new `Opts.at`, default 60, with which `open()` seeks before playing.
  - The budget is passed per reference: 100 ms on p16, step 3's, and 200 ms on p50.
  - 10 edits, alternately adding and removing a `delay` on `t02` through the app's `add_effect` Tx.
- **Pass:**
  - every `heard_ms` ≤ 200;
  - no jump back (`Sampler`);
  - G6's step check on the output stays under twice the reference sine's largest step;
  - `created()` unchanged for the with-plugins variant.
- **Printed:** the parts, from `EditTiming` plus a new `prepared_ns`:
  - the session's request, compile, check and verify, not counted;
  - `continues`;
  - prepare;
  - the wait for the callback and the block's frame;
  - one device buffer.
- **The definition:** `heard_ms` counts from `compiled_ns`, which is taken when the session's request is
  done, check and verify included. So the gate is slightly kinder than "after the compile finishes".
  This design reads "the compile" as the session's whole request, and RENDER-25 says so.
- **Expected:** about 20 to 40 ms (*estimate*: prepare under 10 ms, a block, a 10.7 ms buffer), against
  150 to 170 ms for HEAD's build alone.
- **Engine rig twin:** `gates::add_effect --reference 50`, started at 3:00 by a new `--start` option in
  place of `one_minute`, which keeps measuring what the audio thread pays.

### G7b. "The tails of untouched tracks are intact"

- **Engine-rig gate `tails`:** p50, two decks with 128-frame periods, so one fill is one block.
  - Deck A plays from 2:55 and, at 3:00, prepares and commits `t02` with a `delay` added. That is G5's
    edit, and it is audible.
  - Deck B plays the same with no edit.
- **Per-track taps.** After each fill the rig reads each track's exported node from the playing
  renderer: `Renderer::output_of(name)`, the last block's buffer, looked up in `PlanInfo.index`. The
  name is the node each module exports, from the IR's exports joined to the `Flat` by `FNode.name`. This
  is gates-only reading through `Driver::engine()`; no audio-thread code is added.
- **Pass:**
  - for each of the 49 untouched tracks, its tap is bit-identical between A and B on every block from
    2:55 to 3:10, which covers `t03`'s reverb tail, `t01`'s delay echoes and every voice sounding at
    3:00;
  - the last activation's `Stats` say every stateful node of the 49 tracks and the mixer was kept;
  - `Probe.resets` is unchanged and `held_notes` agrees at the end.
- **Negative control:** `--no-keep`, which is `Basis::Keep` with matching turned off so every node is
  fresh, and also HEAD's engine. Both must fail the same check (`t03`'s tap differs right after 3:00). A
  gate that HEAD passes measures nothing.
- **Slice 3 adds the wrapped instrument:** `t02` wrapped in `vca(…, 1)`, which is `std:scale`, an exact
  multiply by 1.0. It is done twice, once through the app's Tx and once as a file save of the wrapped
  text, the agent path.
  - **Pass:** the whole mix (master output) is bit-identical between A and B on every block, fade window
    included. The fade's equality branch makes that exact (§4.6). `t02`'s pluck is in the kept list.
  - **Negative control:** the same edit on slice 2's names restarts the pluck and must fail.

### G7c. "Live playback with no edits is bit-identical to offline"

- **Already a CI test:** `core/play/tests/bit_identity.rs` (`live_is_the_export_on_every_pure_fixture`,
  `live_is_the_export_on_p16`, `a_no_op_swap_changes_no_bit`). It must pass unchanged at every slice.
- **New:** `a_no_op_activation_changes_no_bit`. For every pure fixture and p16:
  - play live from 0;
  - at 5 s and at 2 later points, in the middle of notes, activate a renderer prepared from the same
    revision with `Basis::Keep` and identity `cont`;
  - **pass:** the whole output equals offline bit for bit, and `Stats` says everything was kept, with 0
    fresh, 0 ghosts and 0 fades.

  This is the cheapest proof that moves keep state exactly. It covers schedulers, voice pools, rings,
  audio taps, the cell resync and the kernel wrappers' first-call copies.
- **New variant:** the same, with every notes lane's data rebuilt (new `Arc`s, equal contents) and sent
  in the lane moves. Reconcile against equal data changes nothing.
- **Recorded:** `live_is_the_export_on_p50` and `a_no_op_activation_on_p50` (`--ignored`), since the
  gate names p50 and the store layout may matter at that size.
- **Offline byte-identity:** `core/session/tests/renders.json` and the scratchpad baseline. Every pure
  fixture's offline render stays byte-identical at every slice.

### G7d. "A kernel module rebuilt with the same kernel id but a new hash starts from init"

- **Runtime test `a_rebuilt_kernel_module_starts_from_init`** (`core/runtime/src/tests/live/keep.rs`).
  1. Load std into `PackageSet` A.
  2. Copy std to a temp dir and append a custom section to `std.wasm` (id 0, name `m2-test`): valid wasm,
     the same code, a new sha-256. That is `PackageSet` B.
  3. Play a one-note song through `std:delay` with feedback. Mid-echo, activate the same `Flat` prepared
     with B and `Basis::Keep`.
  4. **Pass:**
     - `Stats` says 0 std kernels were kept;
     - after the fade the echo is gone, because the new delay starts empty;
     - the output differs from the control twin's by more than −60 dBFS.
- **Control:** a `PackageSet` C loaded from the original bytes, which is a new wasmtime `Module` with the
  same hash, keeps every instance, and its output is bit-identical to never activating. This proves that
  the shape compares content, not objects.
- **Why it is runtime-level:** the app loads packages once per session. A reload path isn't part of
  step 7 (risk R8).

### G7e. "An export of `examples/with-plugins` pauses playback, reuses its instances, and playback resumes with their state intact"

- **Server gate `export`** (`--server`), skipped without Surge XT and Airwindows on `CLAP_PATH`, as
  `with_plugins` is. Copy the fixture, open it, play from 0:02, then start the export through
  `m2_server::export::start`.
  - **Pauses:** while it runs, the published status is `paused: "export"`, the player reports the `Live`
    lent, and the engine clock doesn't move.
  - **Reuses:** `created()` is unchanged across the export, and the plan's plugin handles before and
    after are the same `Handle`s.
  - **State intact:** `save_state` on each handle before and after gives equal bytes.
  - **Resumes:** playing again within 50 ms of `done`, at the beat it held, with the output above −60 dBFS
    within 2 buffers, and no step over G6's bound at the resume.
  - **The file:** a WAV of the song's length. Plugins aren't bit-exact across instances, so its RMS per
    second and onsets are compared with `m2 render`'s (fresh instances), within 1 dB.
- **CI test with the probe plugin** (`core/plugins/src/probe.rs`, no Surge needed), in
  `crates/server/src/graph/tests.rs`: a project using `clap:m2.probe` with a saved state.
  - The probe's call log shows, in this order and on the plugin main thread:
    1. `save_state`;
    2. `deactivate`;
    3. `load_state` (the committed blob);
    4. `set_render_mode(Offline)`;
    5. `activate`;
    6. then, after the render, `deactivate`, `set_render_mode(Realtime)`, `load_state` (the saved
       state) and `activate`.
  - `reset` is called on the export thread after the first `activate`.
  - No `process` call comes from the audio thread between the first `deactivate` and the last
    `activate`.
  - The export with borrowed instances equals the export with fresh instances bit for bit. The probe is
    deterministic, so this proves that `reset` plus `load_state` give RENDER-1's exact start.
- **A pure song** (`examples/first-song`) exports beside playback: no pause, no state call.

### G7f. "The per-instance Store cost at 6,176 instances is measured, and the store layout is chosen from it"

See §7.2. The report prints:

- the measured instance count;
- the table of layouts against metrics;
- the rule's outcome;
- after slice 2, the arena's p99 at p50.

### Gates that keep passing

- Every step 3 gate: G1 to G7, G10, with-plugins, G8, G9 and G11.
- The with-plugins checks of "carried instances" become "kept by the arena", with the same assertions
  (`created()` unchanged, notes go on). `--no-carry` is renamed `--no-keep`.
- `core/play/tests/no_alloc.rs` runs one activation that does all of these under the allocation trap,
  with cell writes from another thread:
  - moves kernels, polys, two probe plugins, rings, audio taps and schedulers;
  - grows a lane;
  - makes ghosts and fade nodes;
  - releases notes;
  - ends a fade and retires the leftovers;
  - refuses a stale activation;
  - swaps a lane mid-fade;
  - panics mid-fade.

  It also asserts `kernel_memory_bytes` is unchanged for the kept instances.

---

## 9. Build order: four slices, each landing green

Coordination: the data-plane workflow is editing `core/session`, `crates/server`, `crates/gates` and
`web` now. Slice 1's runtime and play work can start at once. Every server, gates, session and web
change waits for that workflow to land, and rebases on it.

### Slice 1: measure the Store, make instances movable (about 1.5 weeks)

There is no change in behaviour. Commits, in order:

1. **G7f part 1** (`crates/gates/src/store.rs`), with the layout decided by §7.2's rule. Optionally
   enable wasmtime's `pooling-allocator` feature for D.
2. **`KernelInst` with its own `Store`.** `KernelNode` splits into a wrapper (`inputs`, `seen`,
   `consts_written`) and `inst: Option<Box<KernelInst>>`. The `store` argument goes from every `Graph`
   function.
3. **`PolyNode`** splits into a wrapper and `pool: Option<Box<VoicePool>>`. `Exec::Capture(PortRef)`
   becomes `Exec::Capture(u16)` into `PolyNode.captures`, and voice `Graph::process` takes
   `(outer, captures)`.
4. **`AudioNode` holds `Arc<AudioLane>`.** `LaneSlot::Audio` is deleted, and `lane_slots` lists notes and
   control lanes only.
5. **Engine-wide note ids.** `Renderer.next_note` is passed to `scheduler.run`, and `enter_from` copies
   it. Delete `NoteScheduler.next_id`, `next_id()`, `raise_next_id`, `PluginInstance.next_id`,
   `held_cap` and `hold`. `carry_from` stops raising ids, since the counter is shared.
6. **The `Plan` and `Lanes` structs** inside `Renderer` (§2.2), with `plan.rs` holding `Shape` and
   `PlanInfo`. `prepare(.., Basis::Fresh)` is the only basis, and `build_live` stays as a wrapper that
   still takes the `InstanceSource`.

Lands green because:

- every existing test passes unchanged: `bit_identity`, `no_alloc`, `live.rs`, `carry.rs`, `cell.rs`,
  `tests.rs` and the gates;
- offline renders are byte-identical;
- the step 3 gates pass at p16;
- `prewarming_keeps_kernel_memory_flat` and `kernel_memory_bytes` hold with per-instance stores.

Size *estimate*: +600 / −350 lines in `core/runtime`, about 40 in `core/play` tests.

### Slice 2: the arena (about 2 weeks)

- **`plan.rs`** (about 700 lines):
  - `Basis::Keep`;
  - the matching, edges, ghosts, fade nodes and Kahn order of §4.3;
  - `Entry` and `Stats`;
  - `Renderer::take_over` and `end_fade`;
  - `take_from` per kind;
  - the `Exec::Fade` process;
  - the free-running ghost `AudioNode`;
  - the voice-cell resync.
- **`engine.rs`:** `try_apply` calls `take_over`. Delete `Fade`, `fading`, the fade mixing in `render`,
  the coalescing branch, `heard[1]`, and the panic and transport arms for the fading renderer. Add
  `end_fade` at block lines, and `Retired::Nodes`.
- **`sched.rs`:** delete `adopt`, `adopted`, `released` and `release_all`. Add `take_state` for `grow`.
- **`plugins.rs`:**
  - delete `CarryKey`, `Carry`, `InstanceSource`, `Fresh`, `carry_from` and `lend`;
  - `PluginInstance` keeps `host`, `h`, `rate` and its `Drop`;
  - `PluginNode.lent` and `PluginView.lent` go;
  - `render_block_lent` and `render_block`'s `release_all` go;
  - delete the runtime's `in_voice` plugin branch and `Builder.ordinals`/`source`.
- **`core/session`:** `tx/continues.rs` (§3.1) and `Graph.ids` in `delta.rs`, about 80 lines.
- **`crates/server/src/graph/actor.rs`:**
  - `Plays.info: Arc<PlanInfo>` replaces `slots`, `caps`, `cell_slots` and `cells`, and `Open.carry`
    goes;
  - `reach` calls `continues` and keeps the delta;
  - `way` prepares with `Basis::Keep` for `Kind::Activate` and `Basis::Fresh` for `Replace` and `Load`;
  - the `Way` variants lose their `Carry`;
  - `heard_plugins` fills `restarts`;
  - notes lanes that outgrow their room go to `Kind::Activate`, which is now cheap.
- **`crates/gates`:**
  - `engine.rs` prepares with `Basis`;
  - `Lab.carry` becomes `keep` (`--no-keep`);
  - `Probe.{kept, fresh, ghosts, fades}`;
  - `Renderer::output_of`;
  - the `tails` gate (G7b);
  - `--at` for the server gates (G7a);
  - G7f part 2.
- **Tests:** `core/runtime/src/tests/live/carry.rs` is rewritten as `keep.rs` (§10), plus G7c's and
  G7d's tests and `no_alloc.rs`'s extended run.

Lands green because:

- the gates G7a, G7b (untouched tracks), G7c, G7d and G7f pass;
- every step 3 gate passes at p16 and p50;
- `a_failed_build_then_a_lane_edit_rebuilds` and the lease tests pass on the new `Plays`.

Size *estimate*: +1,300 / −700 lines.

### Slice 3: names that survive edits (about 1 week)

- The fixes F1, F2, F3, F5, F6 and F7 (§3.2), in `core/session/src/text.rs`, `core/script/src/sites.rs`,
  `core/session/src/tx/delta.rs` and `continues.rs`.
- Optional: the editor's text carry is unified with `Edits::between`, and `m2_ir::control_id` is
  deleted.
- G7b's wrapped-instrument variant (Tx and file save).
- The ID-7, ID-13, ID-14, ID-15, WRITE-8, WRITE-12 and WRITE-22 contract edits.

Lands green because:

- step 6's tests pass, `delta_names_are_unique_on_every_fixture` with its new spelling;
- the two-process delta hash test passes;
- the expectation tests are unchanged, because they judge by spans;
- `continues`' tests and the new delta tests pass (§10).

Size *estimate*: +300 / −120 lines.

### Slice 4: plugins persist (about 1.5 weeks)

- **`LoadState`** (§6.2):
  - actor: state changes become `load_state` on continuing instances;
  - `PluginInstance.reloaded`;
  - the latency warning;
  - `Shape::Plugin` loses `state`;
  - the `inprocess.rs` `last_saved` fix.
- **Export** (§6.3):
  - `Player::lend` and `give_back`, with the device fade-in;
  - `Basis::Borrow` and `reset_first`;
  - `Actor::export` and the export thread's protocol with its guard;
  - `Open.lent`, with deferred settles and transport routes refused;
  - `ExportDone`.
- **"loading <plugin>"** and **"paused for export"** in the engine view and the web status area (§6.4).
- **G7e,** both the server gate and the probe CI test.
- **Contract:** the RENDER-1, RENDER-25, RENDER-26b, HOST-5 and HOME-6 edits.

Lands green because G7e passes, with-plugins G5 and G7 pass, `core/plugins` tests pass, and
`main_thread.rs` passes with the probe.

Size *estimate*: +550 / −80 lines.

---

## 10. Tests

### 10.1 New tests, by crate (each names its rule in a comment)

`core/runtime/src/tests/live/keep.rs` replaces `carry.rs`. Its tests:

- RENDER-25:
  - `an_activation_moves_every_instance_that_continues`;
  - `a_changed_shape_starts_from_init`, for a channel count and for a kernel id;
  - `a_rebuilt_kernel_module_starts_from_init`, the G7d test, with its same-bytes control;
  - `a_failed_prepare_leaves_what_plays_untouched`.
- RENDER-25a:
  - `an_inserted_effect_fades_dry_to_wet`, which checks the fade window's samples against
    `o + (n − o)·g`;
  - `a_removed_effect_rings_out_as_a_ghost_then_goes`, where the ghost is retired at the window's end
    and `Retired::Nodes` holds it;
  - `a_silent_edit_changes_no_bit`, the bit-equality branch.
- RENDER-26a:
  - `a_replaced_instrument_releases_its_notes`;
  - `a_kept_consumer_keeps_its_notes`, which replaces `a_carried_instance_keeps_its_notes`;
  - `a_rewired_consumer_releases_its_notes` (F8);
  - `a_lane_that_outgrows_its_room_grows_without_a_restart`.
- `note_ids_never_repeat_into_a_plugin`, which replaces `ids_never_repeat_after_a_renderer_without_notes`.
- Under the names of slice 3 (F7, ID-8): `keyed_instances_follow_their_keys` and
  `a_moved_keyed_plugin_continues`.
- RENDER-28: `an_activation_prepared_against_another_plan_is_refused` and
  `activations_are_never_coalesced`.
- RENDER-27:
  - `a_tempo_change_keeps_every_kernel_and_the_beat`;
  - `a_tempo_change_recomputes_pending_offs`, where a held note's Off lands at its end beat under the
    new map;
  - `an_audio_clip_edit_crossfades_and_keeps_the_rest`, on `examples/resample`.
- RENDER-23: `a_kept_polys_voice_cells_take_the_new_values`, including a lease set before the send.

`core/runtime/src/sched.rs`:

- `take_state_keeps_sounding_notes`;
- the existing swap, wrap and ordinal tests unchanged, and the `release_all` and adoption tests deleted.

`core/play/tests`:

- `bit_identity.rs`: `a_no_op_activation_changes_no_bit` and its lanes-rebuilt variant, and the ignored
  p50 runs (G7c);
- `no_alloc.rs`: the extended activation (§8);
- `live.rs`: `no_edit_or_jump_leaves_a_note_stuck` and `random_edits_and_jumps_never_strand_a_gate`
  extended with random structural edits (insert, remove or replace an effect; replace an instrument)
  between jumps, which holds RENDER-26a under the arena;
- the export protocol: `a_lent_live_comes_back_and_fades_in`.

`core/session` (slices 2 and 3):

- `continues`: `an_insertion_above_continues_everything` (step 6's gate edit),
  `a_label_rename_continues_everything` (ID-10), `a_loop_that_grows_resets_its_occurrences` and
  `a_keyed_loop_keeps_through_reorder_and_growth` (ID-14), `a_moved_keyed_plugin_continues` and
  `adding_a_plugin_key_continues_its_instance` (ID-8, F7), `untouched_files_continue_whatever_changed`;
- `text.rs`: `token_hunks_are_an_edit_script` (a property test: hunks applied to old give new, sorted,
  disjoint, on character boundaries), `two_literals_on_adjacent_lines_are_two_hunks` (F1),
  `a_reformat_changes_no_token`;
- `delta_tests.rs`, all through `delta_between`: `a_wrap_by_a_file_write_keeps_the_wrapped_names` on the
  p50 `t02` text plus nested `reverb(pad(…))` → `reverb(delay(pad(…)))` and the unwrap (F2),
  `adjacent_value_edits_are_values` (F1), `a_component_swap_at_one_call_is_new_and_an_alias_is_not` (F3,
  PKG-9), `names_ignore_everything_above_a_key_frame` (F6), `old_side_names_are_own_offsets`.

`core/plugins`:

- `a_restored_state_loads_after_another` (HOME-6 fix);
- `a_state_load_resends_code_owned_parameters` (HOME-4).

`crates/server/src/graph/tests.rs`:

- `a_state_change_loads_into_the_running_instance`: no new instance, and `created()` unchanged;
- the G7e probe export test;
- `edits_during_an_export_play_when_it_ends`;
- `transport_is_refused_while_exporting`;
- `a_tempo_change_keeps_every_instance`.

### 10.2 Existing tests whose meaning changes

| Test | Change | Rule |
|---|---|---|
| `activations_crossfade_without_jumping_back` (`core/runtime/src/tests/live.rs`) | the fade is per edge; still no jump back | RENDER-25a, RENDER-8 |
| `activations_while_a_voice_sounds_dont_click` | a kept voice now keeps sounding, where today it was released; the click check is unchanged | RENDER-25 |
| `structural_messages_keep_their_order` | consecutive activations no longer coalesce | RENDER-28 |
| `a_changed_state_is_a_new_instance_and_the_old_one_goes_with_its_renderer` | slice 2: unchanged; slice 4: becomes "a changed state loads into the same instance" | RENDER-26b |
| `a_fresh_instance_chases_nothing`, `a_lane_that_shrank_lets_go_of_what_doesnt_fit`, `notes_the_edit_removed_are_let_go` | ported to `keep.rs` with the same assertions | RENDER-26a |
| `a_failed_build_leaves_its_carry_whole` | becomes `a_failed_prepare_leaves_what_plays_untouched` | RENDER-25 |
| `sched.rs` adoption and `release_all` tests | deleted with what they test | RENDER-26a (C7 withdrawn) |
| gates `--no-carry` | `--no-keep`, and the negative reference for G7b | RENDER-25 |

---

## 11. What is deleted

| Deleted | Where | Replaced by |
|---|---|---|
| `CarryKey` (with its ordinal and state hash), `Carry`, `InstanceSource`, `Fresh` | `core/runtime/src/plugins.rs` | `continues` + `Shape` + moves; `Basis` |
| `carry_from`, `lend`, `Renderer::carry_from`, `render_block_lent` | `plugins.rs`, `lib.rs` | `take_over` |
| `PluginInstance.{key, restart, held_cap, next_id}`, `hold`, `mark_restart`, `Carry::restart` | `plugins.rs` | `Shape::Plugin`, the actor's `restarts`, engine-wide ids |
| `PluginNode.lent`, `PluginView.lent` | `lib.rs` | nothing: no instance is processed twice |
| `Builder.ordinals`, `Builder.source`, the `in_voice` plugin branch | `lib.rs` | moves; the checker already refuses plugins in `poly` |
| the engine's `Fade`, `fading`, the fade mixing, `fading()`, the coalescing branch | `core/runtime/src/engine.rs` | `Exec::Fade` and ghosts in one plan |
| `render_block`'s `release_all`; `NoteScheduler::{adopt, raise_next_id, next_id}`; fields `next_id`, `released`, `adopted` | `core/runtime/src/sched.rs`, `lib.rs` | persistent schedulers; `Renderer.next_note`; `Entry.release` |
| `LaneSlot::Audio`, audio lanes by slot | `lib.rs`, `audio.rs` | `AudioNode.lane: Arc<AudioLane>` |
| `Renderer::build_with`, `build_live` | `lib.rs` | `Renderer::prepare(.., Basis)` |
| `Exec::Capture(PortRef)` with outer indices inside voices | `lib.rs` | `Exec::Capture(u16)` into `PolyNode.captures` |
| the `store` argument on `Graph::{process, reset, prewarm, active, kernel_memory}` and `PolyNode::process` | `lib.rs` | each `KernelInst`'s own `Store` |
| `Open.carry`, `Way::{Activate, Replace, Load}`'s `Carry`, `build()`'s source | `crates/server/src/graph/actor.rs` | `Plays.info` + `Basis` |
| `Plays.{slots, caps, cell_slots, cells}` | `actor.rs` | `PlanInfo` |
| `Lab.carry` (`--no-carry`), `Carry` in the engine rig | `crates/gates/src/{lib,engine}.rs` | `--no-keep`, `Basis` |
| `controls::carry_text`, `carry_ends`, `carry_lines` (slice 3, optional) | `crates/editor/src/controls.rs` | `text::Edits::between(..).carry` |
| `m2_ir::control_id` (slice 3) | `core/ir/src/lib.rs` | the control constant's delta name |
| `tests/live/carry.rs` | `core/runtime/src/tests/live/` | `keep.rs` |
| contract: ID-13's `SiteId(u32)` and its rule 4; the engine shape's `arena: Map<RuntimeId, Slot>`, `LoadState` and `SetArrangement` messages; RENDER-32's "per partition" fallback; step 3b's C7 | `docs/architecture.md` | §12 |

**Unified:**

- activation, a notes lane outgrowing its room, a tempo change, an audio-lane or asset edit, and a
  package change are one `Activate` with moves;
- kernels, voice pools, plugins, rings, audio taps and schedulers all move one way (`take_from`);
- a removed node's ring-out and a replaced note consumer's release are one ghost;
- `LoadState` and export's restore are one `load_state` plus `reloaded`;
- export's reuse is activation's match, sharing `Arc`s instead of moving boxes.

**Kept, though they look like candidates:**

- **`same_layout` and `value_writes`:** the 1-block fast path for value edits. Their fallback is now an
  activation that cuts nothing, so they are no longer load-bearing for state. A name join through
  `continues` could replace them later.
- **`LiveFade` in `core/play`:** opening a project, a rate change, unload and lend.
- **`Player::send`'s 500 ms `Stalled`.**
- **`Renderer::seek`'s offline replay** (RENDER-1).

---

## 12. Proposed contract edits

| Rule | Now (in short) | Proposed | Why |
|---|---|---|---|
| Planes table, Prepared data and Arrangement rows | `SwapLane`, `LoadState`; `SetArrangement` | audio lanes and assets: renders prepared off the audio thread, then `Activate` of the same structure; plugin state: `load_state` on the plugin main thread; arrangement: tables and renders prepared off the audio thread, then `Activate` of the same structure | an activation that keeps every instance is a data switch; one path, one test |
| RENDER-1 | calls: `deactivate`, `reset`, `load_state`, `set_render_mode(Offline)`, `activate` | `save_state`, `deactivate`, `load_state` (committed), `set_render_mode(Offline)`, `activate`, then `reset` on the render's thread before its first `process`; after: `deactivate`, `set_render_mode(Realtime)`, `load_state` (saved), `activate`. "Pauses" = the engine is taken out of the device callback with a 5 ms fade, frozen, and given back with a 5 ms fade-in; edits made meanwhile play when it returns | CLAP's `reset` is an audio-thread call on an active instance (`audio.rs` `reset`); "give back the state they had" needs a save first |
| RENDER-16 | "A voice's RuntimeId is its allocator slot." | "A `poly` is kept or replaced whole; a kept pool's voices keep their slots (a voice's RuntimeId)." | contract-notes proposal 11; 97% of instances are voices, and moving a pool as one box keeps entry O(top level) |
| RENDER-23 *Today* | "cells live and die with their renderer until the arena keeps them across plans (step 7)" | cells stay per plan; leases re-aim at each activation; a kept pool's voice cells are written at entry when they differ | they never needed to persist |
| RENDER-25 | keep instances whose RuntimeId and shape match; shape listed | add: "Matched when the plan is prepared, off the audio thread, against the plan the engine will be playing; `Activate` carries the moves. The kernel's module hash is the sha-256 of its package's wasm module. A `poly`'s shape includes its voice graph's structure; an audio lane node's, its clip renders." Make "after the compile" read "after the session's request (compile, check, verify)". Today: rewrite | where the match runs, and what the gate measures, said plainly |
| RENDER-25a | fade "at least at every module output whose subgraph changed"; old and new run together | "An activation crossfades, linearly over 5 ms, every input of a kept node whose source changed, and the song output when its source changed. A replaced or removed node runs on in the new plan for that window (a ghost), its notes ended, then is retired. Nothing else fades; equal signals stay bit-identical." | fading at module outputs needs kept instances processed twice (lending); edges need neither |
| RENDER-26 | addressed by renderer `generation` and slot | as built; the generation is a check, never a drop | strict order makes it current |
| RENDER-26a | "An activation that replaces a note consumer first sends an Off…" + the C7 paragraph | "Schedulers move from plan to plan with their sounding notes and the engine-wide note ids. An activation that replaces a note consumer, or keeps one whose notes input now reads another lane, ends every note sounding into it. An arrangement switch reconciles every scheduler with its new data, which recomputes each pending Off from its beat." Delete C7's paragraph. | adoption existed only because schedulers died with their renderer |
| RENDER-26b | `LoadState` as an engine message; re-plan on latency change | "…reaches the running instance as `load_state` on the plugin main thread (skipped as HOME-6 says); its code-owned parameters are sent again at its next block (HOME-4). A latency that becomes nonzero raises `plugin.latency-uncompensated` until compensation ships." | CLAP's state load is a main-thread call; there is nothing to re-plan while RENDER-21 refuses latency |
| RENDER-27 | `SetArrangement` | "…switched together at one block boundary by an `Activate` of the same structure, which keeps every instance." | the same path as code |
| RENDER-28 | messages incl. `SetArrangement`, `LoadState`; "coalesces to the newest plan" | messages: `Activate`, `SwapLane`, `Transport`, `Panic`. "Each `Activate` names the plan it was prepared against; activations apply in order and are never coalesced. A sender waits up to 500 ms, then reports the player stalled; the next change opens a new player." Fix the stale first Today sentences. | a move list is relative to one plan; the stall behaviour is what's built |
| Engine shape (step 7) | `Plan`, `DataSet`, `Engine { arena: Map<RuntimeId, Slot>, … }`, six messages | `Renderer { plan: Plan { graph, order_after, cells, out, entry, info }, lanes: Lanes, tempo, arrangement, transport, clock, next_note }`; `Engine { current: Renderer, clock, … }`; `Msg { Activate(Box<Renderer>), SwapLane(generation, slot, LaneSwap), Transport(Cmd), Panic }`; instances live in the plan's stateful nodes and move at activation | the arena is the playing plan's slots; no map, no RuntimeId on the audio thread |
| RENDER-32 | "One `Store` per instance, measured first at 6,176. If that costs too much, one `Store` per partition." | "One `Store` per kernel instance, measured by G7f at p50's measured count. If it costs too much, wasmtime's pooling allocator or a smaller memory reservation, never a shared store (a `Store` frees nothing until it drops, and an instance in one can't move)." Today: the G7f numbers. | partitions don't exist yet, and a partition store would leak replaced instances |
| ID-7 | "pins SiteIds from step 7" | "A `key()` frame is named by its key; nothing positional above the innermost one is part of an InstancePath." | what pinning means, as built (F6) |
| ID-13 | the session gives each site a `SiteId(u32)` at each accepted revision, by 4 rules | "A site's identity across two revisions is its file and the point of its argument list's `(`, in the reference coordinates of the comparison (WRITE-8). Nothing numbers sites. A Tx is judged in its own edits' coordinates; every other comparison, the engine's included, uses `Edits::between`: a line diff whose hunks a token-level Myers diff refines. A `key()` frame is named by its key. An ambiguous or unmatched site is new, and its state resets." Rule 4 moves to the Deferred row "Keyed iteration helpers and better structural matching". | a numbered SiteId in names would make `DeltaHash` depend on session history and break step 6's two-process gate; the engine only ever compares what plays with what is accepted |
| ID-14 | as is | add: "Applied when the engine matches instances; a `key()` frame has no occurrence." | it is built in `continues` (F5) |
| ID-15 | RuntimeId = InstancePath / the site of the `kernel()` or `plugin()` call, its occurrence | "…of the `kernel()`, `plugin()`, `poly()` or `feedback()` call, or the lane read of an audio lane. Segments are `key:<k>` or `<ComponentId>@<file>:<point>#<occurrence>`. A keyed plugin unmatched by RuntimeId matches by its state path." | these hold state too; F3, F7 |
| WRITE-8 | "From step 7, SiteIds replace the mapped ranges"; the delta drives `LoadState`, `SetArrangement` | "each frame its ComponentId and its call's point (ID-13)". The delta drives: values → `SetCell`; lanes → `SwapLane`; states → `load_state`; assets, audio lanes and the arrangement → prepared renders and `Activate`; structure → `continues`, prepare and `Activate`. | as built; folds in step 6's C17 |
| WRITE-12, WRITE-22 | "by line diff" | "by `Edits::between` (ID-13)" | one correspondence (slice 3) |
| HOST-5 | matched by module, id, ordinal, state hash; "From step 7 … by RuntimeId" | "Plugin instances persist across plans like every instance (RENDER-25): by RuntimeId and shape (id, version, channels); a keyed one also by state path. A state change is loaded, not a new instance (RENDER-26b). An instance that asked for a restart is new at the next activation. Offline renders in the app share the live instances (RENDER-1); the CLI creates its own." | the ordinal mismatched instances on insertion |
| HOME-6 | as is | add: "Loading a blob makes it the instance's last-known state, so a later load of an earlier save isn't skipped." | the restore after an export |
| DIAG-3 | "from step 7 the notice also compares SiteIds" | "…compares the point of each module's `track()` call (ID-13)", or stays deferred | there are no SiteId numbers |
| Deferred, "Sending deltas to clients" | "With step 7, or when UI update latency is measured over budget" | "When UI update latency is measured over budget" | a UI concern; it would only enlarge step 7 |
| Acceptance gate 7 | "6,176 instances" | the count G7f measures (about 6,060 by std's sources) | the gate states what it measured |
| Stale Today lines | RENDER-6, 9, 12–14, 17, 25, 25a, 27, 28, 33, HOST-6, HOME-4 (listed in `understand-contract.md` §4) | rewritten to the code in the step 7 commit | the contract's own rule |

---

## 13. Clarifications for the owner

| # | Question | This design's answer |
|---|---|---|
| C1 | Is a `poly` kept or replaced whole? | Whole. An edit inside an instrument's voice graph restarts that instrument, as today; everything else keeps. Voice-level matching comes with mono and legato. |
| C2 | What does "pause" mean during an export with plugins? | The engine freezes, with no tails running, behind a 5 ms device fade out and in. The transport resumes where it was. Edits are taken and played when it ends. The transport is refused meanwhile and the UI says why. |
| C3 | Is SiteId a point or a counter (ID-13)? | A point. A counter would break step 6's two-process hash gate. |
| C4 | `LoadState` and `SetArrangement` as engine messages? | Neither. A main-thread call, and an activation of the same structure. |
| C5 | Where do structural fades fall (RENDER-25a)? | On changed edges into kept nodes and on the output, linear; replaced nodes ring out for 5 ms as ghosts. |
| C6 | RENDER-1's call order? | `reset` after `activate`, with a `save_state` first. |
| C7 | Step 3b's C7 (adoption)? | Withdrawn: schedulers persist. |
| C8 | May activations coalesce (RENDER-28)? | Never; each names its basis. |
| C9 | A plain number edited inside an instrument body? | It is part of the voice shape, so it restarts the instrument. Declared controls are cells and don't. |

---

## 14. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | Per-instance stores at 6,000 or more cost too much: on-demand reservations (4 GiB plus guards each, about 25 TiB at p50) near the address-space ceiling, slow `Instance::new`, or call overhead from scattered stores | G7f measures first and its rule falls back to pooling or a small reservation in the same code shape; the arena itself removes today's 2× peak at every activation |
| R2 | A ghost closure grows to the whole song (a package change, or a master rewire with everything replaced) | 5 ms at 2× DSP, today's cost on every edit; only on rare edits; G7f's p99 run includes one |
| R3 | A kept consumer whose source is a kept node downstream of a removed one reads that node's new output, not its old one, during the window | A kept instance has one output; the fade is still click-free (G6), just not a perfect A/B for 5 ms |
| R4 | A poly's blanked voice compare misses a structural difference, so a pool moves into a graph it doesn't fit | `Voice` `PartialEq` on everything but values, origins, names and capture targets; a debug check at entry that voice graph node counts match; `a_changed_shape_starts_from_init` covers voice changes |
| R5 | Myers misaligns repeated tokens (many identical `kernel(` calls with one deleted), giving a neighbour's state to an instance | Shape bounds it to the same kind; ID-14 counts catch loops; the same exposure ID-14 already accepts for reorders |
| R6 | Export's pause loses something: a transport command sent during the export, or a `LoadState` racing the borrowed instance | The actor refuses transport and defers every engine send while `lent`; the guard always restores and gives back; probe CI test |
| R7 | Delta spellings change in slice 3, so a `Seen(hash)` computed before the upgrade, or a journaled Tx's recorded hash, no longer matches | Hashes are per-check; a stale one is a `Conflict` that rechecks, never a wrong write; noted in slice 3's commit |
| R8 | "A rebuilt kernel module" is only reachable in a runtime test, since packages load once per session | The shape compares content hashes, so any future reload path is safe by construction; G7d tests the rule directly |
| R9 | A plugin misbehaves on `load_state` while active (CLAP allows it; some plugins glitch) | The same call every host makes for presets; the "loading" state shows; a later host process (HOST-3) isolates crashes |
| R10 | The slices collide with the data-plane workflow in `crates/server`, `crates/gates`, `core/session` and `web` | Slice 1 is runtime and play only; later slices rebase after that workflow lands |
| R11 | The edited track still restarts its instrument until slice 3 lands | Slice 3 is one week and directly follows; slice 2 already meets the gate's words (untouched tracks) |
