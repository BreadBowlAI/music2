# Step 7 design, robust-first: the engine

Against `docs/architecture.md` (normative) at HEAD `cbf5eb9`. Read-only: nothing was built or run, and
no cargo command was used. Files that the data-plane workflow is editing (`core/session`,
`crates/server`, `crates/gates`, `web`) were read as HEAD has them (`git show HEAD:…`). The sources are
the three survey notes in this folder: `understand-runtime.md` (the engine at HEAD),
`understand-identity.md` (identity at HEAD) and `understand-contract.md` (rules, stale Today lines,
gate plans). This design cites their findings and doesn't repeat them.

Step 7's line in the order of work:

> `Plan` split from `DataSet`; SiteId matching, InstancePath and RuntimeId. The arena, with prepare and
> activate, keeping instances whose RuntimeId and shape (wasm hash included) match. Arrangement
> switches prepared off the audio thread; persistent CLAP instances behind `PluginHost`, with
> `LoadState`, and offline renders that reuse them.

Gate:

> In p50, a structural edit at 3:00 is audible within 200 ms after the compile, and the tails of
> untouched tracks are intact. Live playback with no edits is bit-identical to offline. A kernel module
> rebuilt with the same kernel id but a new hash starts from init. An export of `examples/with-plugins`
> pauses playback, reuses its instances, and playback resumes with their state intact. The
> per-instance `Store` cost at 6,176 instances is measured, and the store layout is chosen from it.

**How this design was made.** It starts from what a producer would hear go wrong. For each failure it
picks a rule that makes the failure impossible by how the code is built, not just untested. Then it
removes every mechanism those rules make redundant. Section 1 lists the failures; every later section
names the failure it rules out.

---

## 0. The design on one page

1. **One plan plays at a time, and instances move between plans.** A `Plan` is the structure built
   from `Flat`: nodes, order, buffers, cells and wiring. The instances that hold state (wasm kernels,
   plugin instances, `poly` voice pools, feedback rings) are boxes inside the plan's slots. An
   activation moves each kept box from the old plan's slot into the new plan's empty slot, at a block
   line. Nothing is copied, rebuilt or processed twice. The *arena* is just "the instances in the plan
   that plays".
2. **All matching happens off the audio thread.** Identity is step 6's `GraphDelta` node names,
   compared between the revision that plays and the accepted one, with four fixes (section 2). A node
   is kept when its name, its ID-14 occurrence counts and its shape all match the old node's. The shape
   includes the package's wasm SHA-256. The audio thread receives a move list of old slot index → new
   slot index. Before moving anything it checks a 64-bit shape hash for each move and the id of the
   plan the list was made against. If any check fails, it refuses the whole activation.
3. **For 5 ms the engine plays the union of the two graphs.** An instance both graphs share is one
   instance, processed once. Replaced nodes run on as *ghosts* inside the new plan, with their notes
   released. Every input whose source changed is crossfaded from the old source to the new one.
   Nothing else is faded. An untouched node keeps its inputs, so no fade ever touches it, and its
   output is bit-identical to a run with no edit. This replaces step 3a's whole-renderer crossfade,
   step 3b's lending, and the 2× DSP cost of every activation.
4. **Lanes are engine state, keyed by `LaneAddr`.** Note schedulers (with their sounding notes),
   control lanes and audio players (with their crossfade taps) persist for the engine's life and move
   between plans like instances. Note ids are unique across the whole engine. This removes adoption,
   `raise_next_id`, `release_all` and the stuck-note cases of a rewired consumer.
5. **One message carries every prepared change.** A code edit, a tempo change, a committed value, an
   outgrown scheduler or a plugin restart is an `Activate` that keeps every instance it can. Notes,
   control and audio lane data use `SwapLane`. `LoadState` is a main-thread host call. Export uses a
   new `Hold` message. There is no `SetArrangement` message and no `LoadState` message.
6. **Export holds the engine and borrows its plugins.** `Hold(true)` fades the output out over 5 ms
   and stops all processing. The `held` flag is published only after that callback returns, which
   proves no thread is in a plugin's `process`. The export thread then runs RENDER-1's host calls on
   the live instances, renders, restores each instance's saved state and realtime mode, and sends
   `Hold(false)`. Kernels in the export are fresh.
7. **One wasmtime `Store` per instance**, unless the `store` gate (G7f) says otherwise. A Store frees
   nothing until it is dropped, so any long-lived shared Store leaks every instance it replaces.
   Fallbacks are engine configuration (the pooling allocator, a smaller memory reservation), never a
   shared Store.

What goes (section 13): `Carry`, `CarryKey`, `InstanceSource`, `Fresh`, `carry_from`, `lend`,
`render_block_lent`, the two-renderer `Fade`, activation coalescing, `release_all`,
`NoteScheduler::{adopt, raise_next_id}`, `PluginInstance::{key, held_cap, next_id, hold}`,
`Builder.ordinals`, the dead plugin-in-voice branch, the server's `same_layout`, `value_writes` and
`Open.carry`, and the ID-13 `SiteId(u32)` allocator, which was never built.

Contract edits, by rule id (section 14): ID-7, ID-13, ID-14, ID-15, WRITE-8, RENDER-1, RENDER-10,
RENDER-23, RENDER-25, RENDER-25a, RENDER-26, RENDER-26a, RENDER-26b, RENDER-27, RENDER-28, RENDER-32,
the engine shape, HOST-5, the `PluginHost::load_state` doc, DIAG-3 and one Deferred row. Most of them
make the contract simpler.

---

## 1. What a producer would hear, and what rules each failure out

| # | Heard | How it happens (today, or in a naive arena) | Ruled out by (where) | Checked by |
|---|---|---|---|---|
| F1 | A reverb or delay tail cut on a track nobody touched | Today: every structural edit rebuilds every instance (`Renderer::build_live`). In a naive arena: names that shift (line hunks, a call named by its last `)`), a positional chain above a `key()` frame, or matching that runs on the audio thread and gives up | Names compared between what plays and what is accepted, in the old side's own byte offsets (§2). Kept instances move and are never rebuilt (§5.3). An untouched node keeps its inputs, so no fade touches it (§4.3) | G7b, bit for bit; `an_insertion_above_continues_everything` |
| F2 | A click at the moment of the edit | A switch in one sample; a fresh node's first sample; a kept node whose input switches source | Every input whose source changed is crossfaded over 5 ms, and replaced nodes run on as ghosts for that window (§5.4). A fresh subgraph always meets the kept graph at a faded edge (§4.3, the boundary lemma) | G6 `step`; G7b's bound on the fade window; `a_removed_effect_rings_as_a_ghost_then_retires` |
| F3 | A note that never stops | A kept voice pool reading a lane whose scheduler no longer knows its notes (identity note F8); a scheduler replaced under a kept consumer; note ids that collide in a plugin | Schedulers persist by `LaneAddr` for the engine's life (§6.1). A `poly` whose lane changed counts as a different shape, so it is replaced and its gates close. A plugin whose lane changed lets go of its keys. Note ids are unique across the engine | G4 `stuck`; `random_edits_and_jumps_never_strand_a_gate`, extended with structural edits |
| F4 | A plugin loses its sound (patch, parameters) | A new instance where an old one should continue (ordinal shift, changed state hash); `load_state` skipping a load it must do; code-owned parameters not re-sent after a load; export leaving an instance in offline mode or with the export's state | Plugins move by RuntimeId, then by state path (§2.4). A changed blob is `load_state` into the kept instance, never a new instance (§7.2). The host tracks the hash of the state an instance *holds* (§7.2). A reassert flag re-sends code-owned parameters. Export restores the instance's own saved state and realtime mode (§8) | G7e; `a_state_change_loads_into_the_kept_instance`; `load_then_restore_is_never_skipped` |
| F5 | The export differs from what was committed | The live engine processing an instance while the export does; no reset; still in realtime mode; a `LoadState` arriving during the render | `Hold` proves the audio thread is off every instance before any host call (§8.1). The actor sends nothing to the engine while an export borrows instances. Host calls run in a fixed order with a reset on the render thread. Kernels are fresh | The probe export with reused instances equals one with fresh instances, bit for bit; no audio-thread `process` call inside the hold |
| F6 | A rebuilt kernel plays from stale memory | Matching by kernel id alone | The shape includes the package's wasm SHA-256, channels, ABI and rate. Prepare checks it, and the audio thread checks a 64-bit shape hash again before any move (§5.2) | G7d, with a control from identical bytes |
| F7 | A dropout at the moment of the edit | Today both renderers run whole for 2 blocks (2× DSP); allocation; a storm of `munmap` calls contending with the audio thread | One plan on the audio thread; only replaced nodes run twice (§5.4). Moves and retirements use room allocated off the thread (§5.6). The dropper frees in batches (§5.6) | `no_alloc.rs` extended; G7b's activation-block time; G7f's drop contention |
| F8 | Memory creeping up over a session | A shared Store that never frees; retired objects leaked (`mem::forget`); shells never dropped | One Store per instance (§9). An activation applies only once the room for everything it will retire is free (§5.2). Live-instance counters | G7g `churn` |
| F9 | A neighbour's echo on the wrong part | An ordinal shift; a loop that grows handing state to the next element (ID-14) | Names never use ordinals. Occurrence counts are compared. An ambiguous match gets a fresh instance. Shape gates every keep (§2) | `a_loop_that_grows_resets_its_occurrences` |
| F10 | Playback jumps, or the edit lands late | Rebuild time, a transport copied from renderer to renderer | Prepare is O(edit). The transport belongs to the engine and is never copied. Activation happens at the engine's own block line (§5.5) | G5 and G7a `Sampler`, `heard_ms` |
| F11 | An edit never arrives, or the engine stays on an old plan | A dropped `Activate`; coalescing past a plan that move lists are relative to | Activations are never coalesced. Each names its base plan, and one whose base isn't playing is refused and reported, and the actor prepares again (§5.2) | `an_activation_against_another_plan_is_refused` |
| F12 | A kernel that crashed comes back | Keeping an instance whose wasm trapped | A trapped instance is poisoned and never matches (§5.7) | `a_trapped_kernel_is_never_kept` |

Three rules carry most of this:

- **Verify everything, then move everything.** An activation is atomic on the audio thread: it either
  applies completely or not at all.
- **The plan owns wiring; the arena owns state.** Only state moves. Wiring is built fresh off the audio
  thread for every plan. So nothing on the audio thread patches an index, except the few voice captures
  of a kept `poly`, which come from a prepared list.
- **A boundary always fades.** Kept nodes with unchanged inputs are never faded; every other place where
  new meets old is.

---

## 2. Identity: SiteId, InstancePath and RuntimeId

### 2.1 Decision: names, not numbers

ID-13 asks for a `SiteId(u32)` that the session assigns at every accepted code revision. This design
builds none. The reasons, from `understand-identity.md`:

- The engine only ever compares two revisions: **what plays** (`Plays.revision`) and **what is
  accepted**. Step 6 already names every node between any two revisions (`delta_between`), and on the
  old side every point is `Base(own offset)`. So the old side's names depend on that revision alone and
  can be cached with it.
- A session-assigned counter would be history-dependent state. It would have to stay consistent across
  failed builds, undo and reopen. If it went into delta names (WRITE-8's "From step 7, SiteIds replace
  the mapped ranges"), `DeltaHash` would differ between two processes. That breaks step 6's gate and
  `Seen(hash)`.

So the derived ids are spelled from the delta names:

- **SiteId** = (file, the point of the call's opening `(`) in the comparison's coordinates.
- **InstancePath** = module key, then the frame chain. A `key()` frame is `key:<k>`. A component frame
  is `<ComponentId>@<file>:<( point>#<occurrence>`. Everything above the innermost `key()` frame is
  dropped: that is what "key() pins" means (ID-7).
- **RuntimeId** = InstancePath / `<op>@<file>:<( point>` / `#<n>`, for kernel, plugin, `poly` and
  feedback nodes. A voice's RuntimeId is its allocator slot, kept by moving the voice pool whole (§3.3).

### 2.2 The four fixes the engine needs before names can key the arena

These are on the revision-to-revision path, which step 6's Tx tests don't exercise.

1. **A token-level refinement of `Edits::between`** (`core/session/src/text.rs`). Keep the line Myers
   diff. Inside each line hunk, run the existing generic `kept_lines` over tokens: identifiers and
   numbers, strings, single punctuation characters, with whitespace and comments falling into the gaps.
   Emit one hunk per gap between kept tokens whose bytes differ, trimmed by common prefix and suffix.
   Past `DIFF_LINES` tokens the hunk stays whole. This fixes F1 in the identity notes: two literal
   edits on adjacent lines would otherwise rename the calls between them and cut a tail on a value edit.
   About 80 lines.
2. **Name a call by its `(`.** Add `Site.open: u32` in `core/script/src/sites.rs`
   (`visit_call_expression`), scanning from the end of the callee span (or of the type arguments) past
   whitespace, comments and `?.`. `Side::place` and `Side::spell` (`core/session/src/tx/delta.rs`) use
   `open` instead of `start..end`. A wrapped call's `(` is a kept token whatever Myers does with the run
   of `)`. This fixes F2: wrapping a ringing pluck in `delay(…)` would otherwise rename pluck and cut
   its notes. `Facts.spans` still holds the whole range, so judging `Confined` is unchanged. About 30
   lines.
3. **Put the ComponentId in chain segments** (`Side::chain`). With names by `(`, a component swapped at
   one call (`delay(` → `chorus(`) must still be a new frame. An alias of the same component
   (`import { delay as echo }`) correctly stays the same (PKG-9). About 5 lines.
4. **Compare occurrence counts (ID-14).** `Facts` gains `counts: Vec<u32>`: for each chain segment that
   has an occurrence, how many sibling frames share its (parent, site), then the size of the node's own
   `#n` group. Equal names with different counts don't continue. About 25 lines.

Plus the `key()` pin in `Side::chain`: drop the positional segments above the innermost `key:` segment.
Keys are unique per parent key frame (`keyFrames` in `core/script/sdk/core.ts`), so those segments are
redundant. About 10 lines.

The delta hash changes its spelling once. It stays deterministic, so step 6's two-process gate holds
after its expected names are updated. A Tx checked by an older build and committed by a newer one gets
a `Conflict` on `Seen`. That is acceptable for one release.

### 2.3 Continuity: what the session hands the runtime

New, in `core/session/src/tx/continuity.rs`:

```rust
/// For each top-level node of the new revision's `Flat`: the node of the old revision's `Flat` it
/// continues, by name (ID-13, ID-15), with equal occurrence counts (ID-14). Keyed plugins left
/// unmatched are then paired by module and state path (ID-8). Nothing here knows about shapes:
/// the runtime decides keeping.
pub struct Continuity {
    pub top: Vec<Option<u32>>,
}

pub fn continues(old: &Revision, new: &Revision) -> (GraphDelta, Continuity);
```

- **The join.** Delta names are per IR node. `Side::scope` already builds `ids: HashMap<node id, Name>`
  for each scope; expose that map. A top-level `FNode` is named `<module>/<id>` (`check.rs`, `FNode`),
  so the join is new flat index → (module, id) → name → old name → old (module, id) → old flat index.
  Nodes inside voices aren't joined: a `poly` is kept or replaced whole (§3.3).
- **Stateless nodes are mapped too:** consts, lanes, time and `Missing`. The runtime uses the map to
  decide whether an input's source "is the same source" (§4.3).
- **Second pass, keyed plugins.** An unmatched new `FOp::Plugin` with `state: Some(path)` takes the
  unmatched old one in the same module with the same path. `plugin.state-collision` makes this
  unambiguous. This keeps what `CarryKey.state_path` gave: a moved keyed plugin keeps its instance.
- **Cache.** The old side's named graph is cached on `Code` (a `OnceLock<Arc<side::Graph>>`), since it
  depends only on the code and the snapshot's texts. Each settle names only the new side, about 3,000 to
  4,000 IR nodes at p50 (*estimate*). That cost is measured inside G7a.
- **One walk.** `continues` returns the delta as well, so the actor's `reach` and the matching share one
  naming pass. Today `reach` already calls `delta_between` on every settle.

### 2.4 Why this is safe: the matching's properties

- **Injective.** Names are unique (the `debug_assert` in `scope`, and
  `delta_names_are_unique_on_every_fixture`). The second pass takes only unmatched nodes on both sides.
  So no old instance can be handed to two new nodes. The runtime checks this again: a move list whose
  `from` repeats is refused in `prepare`, with a test.
- **Never across modules.** Every name starts with the module key. (A re-keyed module, step 8b's key
  rename, can be added later as a third pass over `ModuleChange::Rekeyed`. It isn't in step 7.)
- **Shape-gated in the runtime.** A wrong name match can only pair same-kind, same-shape instances.
- **Ambiguity resets.** A changed count resets the whole group. A name with no partner is fresh.
- **Which way the errors lean.** A false *keep* gives a same-shape neighbour's state for a few seconds,
  and its consumers' inputs still fade, so it can't click (§4.3). A false *reset* cuts a tail, which is
  what the gate checks. The fixes in 2.2 remove the false resets on every gesture the app makes and on
  ordinary hand edits. The remaining false resets are a reformat of a whole track call (only that
  track resets) and a cut-and-paste of a block (deferred, with ID-13's rule 4).

---

## 3. Plan and Data: the split, and instance state against plan wiring

### 3.1 Types (new `core/runtime/src/plan.rs`; `lib.rs` keeps the per-node processing)

```rust
/// Unique per plan within the process (replaces `GENERATION`).
pub struct PlanId(pub u64);

/// The structure the engine plays, built from a `Flat` off the audio thread: nodes, order, buffers,
/// cells and wiring, with a slot in each stateful node for its instance (RENDER-25).
pub struct Plan {
    id: PlanId,
    graph: Graph,               // exec, steady order, bufs, versions (as today's Graph)
    union: Option<Union>,       // the activation's 5 ms window: present only when something fades
    moves: Vec<Move>,           // what this plan takes from its base at activation
    cell_moves: Vec<(CellId, CellId)>, // (new, old) for consts that continue: compared at activation
    slots: Vec<Slot>,           // this plan's stateful nodes, for the next plan's moves
    cells: Cells,               // per plan (RENDER-23 stays)
    out: Out,                   // the song's output: a port, or a fade during the window
    rate: u32,
    poison: Arc<Poison>,        // bits set by the audio thread when an instance traps (§5.7)
}

/// The data the engine plays, built from a `DataSet` off the audio thread.
pub struct Data {
    tempo: TempoMap,
    arrangement: Arc<Arrangement>,
    lanes: Vec<Lane>,           // by plan lane index; ghost lanes after `live`
    live: usize,
    moves: Vec<LaneMove>,       // what it takes from the data that plays
}

pub enum Lane {
    Notes(Option<Box<NoteScheduler>>),
    Control(Arc<ControlLane>),
    Audio(Option<Box<AudioPlayer>>),
}

pub enum LaneMove {
    Keep { from: u32, swap: Option<LaneSwap> },   // the old entry, with new data if it changed
    Grow { from: u32 },                           // a notes lane that outgrew its scheduler: sounding notes move into the fresh, larger one prepared here
    Fresh,                                        // built here
}

pub struct Next { pub base: Option<PlanId>, pub plan: Box<Plan>, pub data: Box<Data> }
```

`Engine` then holds `plan: Box<Plan>`, `data: Box<Data>`, `transport: Transport`, `clock: u64`, the
note-id counter, the fade window and the hold state. That is the contract's engine shape, with the
arena as the instances inside `plan`'s slots. The offline renderer is the same pair with a transport
equal to the clock. `Renderer` keeps its name and public API (`new`, `process`, `seek`, `starting_at`,
`render`, `set_strict`, `kernel_memory_bytes`, `resets`) as a thin owner of `(Plan, Data, Transport,
clock)`. So `apps/m2`, `crates/editor/src/render.rs` and the tests change little. Live and offline
call the same `Plan::render_block(&mut Data, &Runs, clock, &mut NoteIds)`, which keeps RENDER-33
(§10, G7c).

**The split, concretely.** `Plan::prepare` reads `Flat` (plus state blobs for plugins it creates).
`Data::prepare` reads `DataSet` and the plan's lane list. A lane data edit touches only `Data`, in place
(`SwapLane`). A code edit builds a `Plan` and a `Data` whose lanes are nearly all `Keep`. A tempo edit
builds a `Data` with new lane data and a `Plan` that keeps every instance. The contract's `Plan { nodes,
order, buffers, cells, latency }` is this `Plan`. `latency` stays empty until compensation, since
RENDER-21's refusal stands.

### 3.2 What moves and what is rebuilt, per node kind

| Node | Instance state: moves at activation | Plan wiring: built fresh for every plan | Shape: must be equal to keep |
|---|---|---|---|
| kernel (top level) | `KernelInstance`: its `Store`, `Memory`, the four typed funcs, port pointers, output channels, shape hash, debug ABI-1 copy | `inputs: Vec<Input>`; `seen: Vec<u64>`, starting at `COPY`, so each input is written into memory once at its first call in the new plan | kernel id, the package's wasm SHA-256, `FOp::Kernel.channels`, ABI version (`m2_abi()`), rate |
| plugin | `PluginState`: `Arc<PluginInstance>`, `keys` (notes sounding into it), `ended`, `sent` (param id → value last sent, HOME-4), `status` | `audio_in`, notes lane index, `params: Vec<(u32, Input)>`, event lists sized from this plan's lane, `release`, `held_room` | plugin id and version, rate; not flagged for restart; not poisoned |
| `poly` | `PolyState`: the voice slots, each a voice `Graph` with its own kernels, note, `born`, `dirty`, `last_peak`; the allocator's `born` | lane index, `out`, `plan` (this block's Ons, sized from the lane's room), and the capture patch list | `voices`, `channels`, the lane's `LaneAddr`, a hash of the voice graph's structure (ops, inner wiring by index, inner kernel shapes, folded constants; not cell values), and each capture's source continuing the old capture's source |
| feedback | `ring: Box<[f32]>` | `input` | channels |
| notes lane | `NoteScheduler` in `Data` | the plan's lane index | `LaneAddr`, kind |
| audio lane | `AudioPlayer` in `Data`: today's `AudioNode` taps, fade-in and continuation, plus the current and previous `AudioLane` data (§6.2) | the plan's lane index | `LaneAddr`, channels |
| control lane | `Arc<ControlLane>` (no state) | the plan's lane index | `LaneAddr` |
| const (cell) | none. At activation the old value is read so a changed value steps or ramps (§5.3) | value, version | name only |
| time, `Missing`, `NoteField`, `Capture` | none | as today | — |

This split is why activation needs almost no patching. `PolyNode` today mixes the two halves (`slot`,
`out` and `plan` are wiring; `voices` and `born` are state), and so does `PluginNode` (`audio_in`,
`notes`, `params`, the event lists and `release` are wiring; `inst`, `keys`, `ended` and `status` are
state). Splitting each into `{ state: Option<Box<…State>>, wiring }` is mechanical.

**`AudioNode` moves into the lane.** Today it lives per graph node (`Exec::Audio`). Two nodes reading
the same audio lane compute the same output, so playing once per lane before the graph (as schedulers
run) gives the same bits. Persisting it by `LaneAddr` means an audio lane's crossfade taps never need
matching by node name. Its comment "nothing is faded at a renderer's first block: the whole-output
crossfade covers it" goes.

### 3.3 `poly` is kept whole or replaced whole

Contract notes, proposal 11: matching instances inside each voice slot of a changed voice graph buys
little, and would need ghosts and fades inside voices. So:

- A `poly` is kept when its name continues and its shape (above) is equal. Its whole `PolyState`
  moves: every voice graph, its kernels, its notes and its cell constants. The voice graphs keep their
  own inner wiring and versions, which stay consistent because the structure is equal. Only each
  voice's `Exec::Capture(PortRef)` holds an outer index; activation sets those from a prepared list,
  the same for every voice.
- Otherwise it is replaced: a fresh `PolyState` is prepared, and the old one becomes a ghost whose
  gates close at the activation frame. The notes on the edited part are released over the 5 ms window,
  as RENDER-26a and owner decision 1 accept.
- RENDER-16's "a voice's RuntimeId is its allocator slot" holds: voice `i` continues as voice `i`. The
  pools that mono, legato and glide will need later persist.

97% of p50's kernel instances are in voice pools (`understand-runtime.md` §2.4). Moving a pool is one
box move, so a kept track costs a few pointer moves at activation however many voices it has.

### 3.4 `PlanIndex`: what the actor knows about what plays

`Plan::prepare` returns `(Next, PlanIndex)`. The actor keeps the `PlanIndex` of the last plan it sent,
in `Plays`:

```rust
pub struct PlanIndex {
    pub id: PlanId,
    slots: Vec<SlotInfo>,         // per stateful node: flat index, kind, shape hash
    by_flat: Vec<Option<u32>>,    // top-level flat index -> slot
    lanes: Vec<(LaneAddr, LaneShape, Caps)>,
    cells: Vec<CellId>,           // by flat const index, for `cell_moves`
    plugins: Vec<(u32, Arc<PluginInstance>)>, // slot -> instance, for LoadState and export
    poison: Arc<Poison>,
}
```

Nothing in it is read from the audio thread. It is what the plan *will* hold once its activation
applies, so the actor can prepare the next plan against it straight away. FIFO order and the base check
(§5.2) make that safe.

---

## 4. Prepare, off the audio thread

### 4.1 Signature

```rust
impl Plan {
    pub fn prepare(
        new: &Flat,
        old: Option<(&Flat, &PlanIndex)>,   // None: a fresh engine, an offline render
        cont: &Continuity,
        packages: &PackageSet,
        rate: u32,
        states: &dyn States,                 // verified blobs by module and state path (DataSet)
        owner: &m2_plugins::Owner,
        lend: &dyn Fn(&FNode) -> Option<Arc<PluginInstance>>, // export only (§8)
    ) -> Result<(Box<Plan>, PlanIndex)>;
}
impl Data {
    pub fn prepare(set: &DataSet, plan_lanes: &[(LaneAddr, Kind)], old: Option<&PlanIndex>,
                   changed: &LaneChanges, cache: &mut audio::Cache, rate: f64) -> Box<Data>;
}
```

`Builder` (`lib.rs:567`) stays as the code that makes fresh instances and nodes. It loses
`source: &mut dyn InstanceSource` and `ordinals`, and creates each kernel in its own `Store`.

### 4.2 Steps

For each top-level node `n` of the new `Flat`, with `o = cont.top[n]`:

1. **Keep or fresh.** A stateful `n` (kernel, plugin, `poly`, feedback) is *kept* when `o` is some
   stateful old node, `shape(n) == old.slots[o].shape`, the old slot isn't poisoned, and no other new
   node already took `o`. Otherwise it is *fresh*: built now (kernels instantiated, `k_init`, prewarm;
   plugins created and activated on the plugin main thread; voice pools built whole).
2. **Wiring.** Every node's inputs are wired to new indices, as `Builder::graph` does today.
3. **Edges (§4.3).** Each kept node's inputs, and the song's output, are compared with the old node's.
   Every changed edge gets a `Fade` node.
4. **Ghosts.** An old stateful node that no new node keeps is *replaced*. It runs as a ghost when it
   reaches an old side of some fade, directly or through other ghosts. The ghost's wiring is its old
   wiring, mapped: an old source that is kept reads the kept node's new index; one that is a ghost
   reads the ghost; a const or cell input becomes `Input::Const(old value)`, which the graph never
   rewrites because the value is already in memory. A replaced node no fade needs is not run at all;
   it is dropped with the old shell.
5. **The union order.** Kahn's algorithm, lowest index first (deterministic), over new nodes, ghosts
   and fades.
6. **The cycle rule.** An edit that reverses an edge between two kept nodes, such as reordering two
   effects, makes the union cyclic. Proof: in old `x→A→B→y`, new `x→B→A→y`, `A` reads `Fade(x, B)`,
   while `B` reads `Fade(A, x)`. If Kahn's algorithm leaves nodes over, every kept node in the leftover
   strongly connected components is demoted to replaced, and steps 3 to 5 run again. This is bounded
   by the number of kept nodes. A demoted plugin is created fresh from its own `save_state`, so it
   keeps its patch (§7.4). The reordered effects lose their tails; nothing else does.
7. **Rewires at the end of the window.** For each faded edge: (node, port, steady input). At the end
   of the window, the engine writes each one and sets that port's `seen` to `COPY`.
8. **Room for retiring.** `union.ghost_box: Box<Ghosts>`, with capacity for every ghost and ghost
   lane, allocated here.
9. **Cells.** `Cells::new(cells(new))` as today. Plus `cell_moves`: for each cell whose const
   continues an old const, (new CellId, old CellId). Plus, for each kept `poly`, its voice cells, which
   are compared at activation (§5.3).
10. **The move list.** `Move { from: old slot, to: new exec index, shape: u64, kind }` for kept nodes,
    and `Ghost { from, to }` for ghosts.

`Data::prepare` does the same for lanes, by `LaneAddr`. A lane that is in both, with an equal shape, is
`Keep`, with `swap` set when the delta lists it or the tempo changed. A notes lane whose new data needs
more room than its scheduler has (`Caps`) is `Grow`. A lane that isn't in the old data is `Fresh`.
Control and audio lanes that the new plan doesn't read but a ghost does are appended after `live` as
ghost lanes. Ghost note lanes aren't needed: ghost consumers read no events.

### 4.3 Which edges fade, and why a boundary always fades

For a kept new node `N` continuing old `O`, input port `p` is **unchanged** iff:

- both are `Input::Port`, the new source node continues the old source node (`cont`), and the source
  ports and channel counts are equal; or
- both are `Input::Const` with equal bits; or
- both are `Unset`.

The song's output is compared the same way. Anything else is a **changed edge**. It gets `Exec::Fade
{ from: Src, to: Src }`, where `Src` is `Port(PortRef) | Const(f32) | Silence`, and `N` reads the fade
node during the window. The output is `(1 − g)·from + g·to`, with `g = (k + 1) / fade_len` over engine
frames from the activation and `fade_len = round(0.005 × rate)`. It is linear, like today's crossfade,
because the two sides are mostly the same music (dry against processed).

- **Boundary lemma.** A fresh node is never on an unchanged edge into a kept node, since a fresh node
  continues nothing. So every fresh subgraph that reaches a kept node or the output does so through at
  least one fade. Fresh instances can start from any init state without a click.
- **Untouched nodes.** A kept node whose inputs are all unchanged reads exactly what it read before. So
  an untouched track is bit-identical through the activation, which G7b checks.
- **What isn't faded:** plugin parameters (a changed parameter source is a parameter step, which
  plugins smooth themselves), event inputs (handled by releases, §6.1), and a changed cell value (it
  steps or ramps as a `SetCell` would, §5.3).

Worked examples (p50, track `t02`, pluck into the mixer's gain):

| Edit | Kept | Fresh | Ghost | Fades |
|---|---|---|---|---|
| wrap pluck in `delay(…)` | pluck's `poly`, gain, everything else | delay | none | gain.in: pluck → delay |
| remove that delay | pluck, gain | none | delay, reading pluck's new output | gain.in: delay → pluck |
| `lowpass(` → `highpass(` at one call | everything but the filter | highpass | lowpass | the filter's consumer's input |
| reorder `comp(eq(x))` → `eq(comp(x))` | everything but comp and eq | comp and eq (demoted by the cycle rule) | the old comp and eq | the consumer's input |
| rebuilt `std.wasm` (G7d) | plugins and lanes only | every kernel and voice pool | every old kernel node | the song's output |

### 4.4 Plugins in prepare

- **New instances** are created, state-loaded and activated on the plugin main thread while the old
  plan plays (RENDER-25). The server publishes `loading: ["Surge XT", …]` in its state for the
  duration, which the app shows as "loading <plugin>" (not built today). The code plane's 200 ms
  budget excludes it, as the contract says.
- **A kept instance whose state blob changed** (`delta.states`) gets `load_state` on the main thread
  before the plan is sent (§7.2). The instance keeps processing meanwhile, which CLAP allows.
- **Restart-flagged instances** (`HostEvent::Restart`) don't match. The fresh one is created from the
  old one's `save_state` (§7.4).

### 4.5 Cost at p50, against the 200 ms

The clock starts at `compiled_ns`, which is after check and verify; the contract says "after the
compile finishes", so the gate is slightly kinder than the words.

| Part | Estimate | Today |
|---|---|---|
| `continues` (new side named; old side cached) | 3–10 ms | `delta_between` is already run on every settle |
| `Plan::prepare` skeleton (top-level exec, bufs, cells) | 1–3 ms | the whole `build_live`: about 150–170 ms at p50 (*estimate*) |
| fresh instances for an added effect | 1 to 3 kernels, under 1 ms | about 6,100 instances |
| `Data::prepare` (lanes kept; only changed ones rebuilt) | about 0 | every `NotesData` rebuilt |
| queue, block line, one device buffer | ≤ 2.7 ms + 10.7 ms | the same |

All of these are estimates; G7a measures each part (§10).

---

## 5. Activation, on the audio thread

### 5.1 Messages

```rust
pub enum Msg {
    /// A new plan and data, prepared against the plan `next.base` names.
    Activate(Box<Next>),
    /// New data for one lane of plan `plan`: notes, control, or audio (prepared clip renders).
    SwapLane { plan: PlanId, lane: u32, data: LaneSwap },
    Transport(Cmd),
    Panic,
    /// Export (§8): fade out over 5 ms and stop processing anything; or resume.
    Hold(bool),
}
```

Transport, `Panic` and `Hold` apply as soon as they are read. While held, `Panic` is deferred until the
resume, so no plugin is touched. `Activate` and `SwapLane` apply in order, as today. **Activations
never coalesce:** each move list is relative to its base. Two in a row apply one after the other, the
second once the first's window has ended (at most 2 blocks at 48 kHz). The `Applied` stamp gains
`refused: bool`.

### 5.2 Readiness and the checks

An `Activate` waits at the head of the queue until all of these hold:

- no fade window is running and the engine isn't held;
- the stash has room for 3 retirements: the old shell now, the ghost box at the window's end, and one
  to spare.

Then **every check runs before anything moves**:

1. `next.base == Some(plan.id)`, or `None` with no plan playing.
2. For every `Move { from, shape, kind, .. }`: `from < old.slots.len()`, the old slot is filled, its
   kind matches, and `old.slots[from].shape == shape`. That last one makes F6 impossible even if
   prepare had a bug.
3. For every `LaneMove::Keep { from, swap: Some(d) }`: the kinds match and `d` fits the scheduler's
   `Caps`.
4. Every `from` appears at most once.

If any check fails, the activation is refused. It is retired whole, unapplied; `refused` is counted
and stamped; the old plan plays on. The actor sees `refused` and prepares again against what plays.
Every check is a compare over a list allocated off the thread.

### 5.3 The moves, in order (no allocation, no drop)

1. **Instances.** For each `Move`: `old.graph.exec[s].take_state()` (leaving `None`) into
   `new.graph.exec[to]`. Then, by kind:
   - *Kernel.* Nothing else to do: wiring and `seen` are fresh.
   - *`poly`.* Write the prepared capture indices into each voice's capture nodes.
   - *Plugin.* If the lane is unchanged and `wiring.held_room <= keys.capacity()`, `keys` stay. If the
     lane changed (identity note F8), every key moves to `wiring.release`, which sends a NoteOff at the
     first call. If the room grew, keys are copied into the prepared, larger `Vec`, and the old one is
     swapped into the old shell.
   - *Feedback.* The ring moves.
2. **Ghosts.** The same move, into ghost slots, then:
   - a ghost `poly` closes every open gate (`note.off = min(note.off, clock)`) and reads an empty event
     slice;
   - a ghost plugin moves its keys to `release` and gets no events after them.
3. **Lanes.** For each `LaneMove`:
   - `Keep`: take the old entry. With a swap, `scheduler.swap(d)` (or `player.swap(d)`, or replace the
     control `Arc`), and the replaced data goes into the old shell's entry.
   - `Grow`: move the old scheduler's `sounding` into the fresh scheduler (a bounded copy into room
     prepared for it), set `swapped`, and put the old scheduler in the shell.
4. **Cells.** For each `(new, old)` in `cell_moves`: if the old plan's value differs from the new
   one, a smooth cell is written as `Write::Ramp(old, new)` and joins `ramping`; any other cell is
   already a step in its fresh buffer. For each kept `poly`'s voice cells: compare each voice const
   with the new table's value, and write only those that differ (ramp or step by `smooth`). This makes
   a committed value edit sound exactly like a `SetCell` (RENDER-22a), with no rewrite of every voice
   input. Top-level kept kernels rewrite their const inputs once, because their `seen` starts at
   `COPY`. That is about 150 kernels at p50.
5. **Transport.** If the tempo map or arrangement changed, `transport.retime(old, new)`. The beat is
   kept, and so are the loop's beats (RENDER-27). Otherwise the transport isn't touched: no `Jump`,
   no `Enter`.
6. **Swap in.** `plan` and `data` are replaced, and the window starts if `union` is present.
7. **Retire** the old plan and data as one `Retired::Shell(Box<Plan>, Box<Data>)`. It holds every
   replaced instance no ghost needed, old buffers, old lane data and old cell slots. All are dropped
   on the dropper thread.

**Bit-exactness of a no-op.** An activation of the same revision keeps everything, has no union, and
writes no cell. Top-level kernels rewrite the same values, schedulers and audio players continue
untouched, and the clock and transport are unchanged. So `a_no_op_activation_changes_no_bit` holds
for pure songs, fade window included. Plugins aren't re-sent their parameters either, since `sent` is
instance state. A no-op is therefore inaudible with plugins too.

### 5.4 The window, and its end

- While the window runs, `Graph::process` follows `union.order`. Fade nodes mix; ghosts process
  (kernels, ghost voice pools releasing, ghost plugins); ghost lanes play (audio players and control
  lanes). **The cost is the song's DSP plus the replaced nodes**, where today it is twice the whole
  song.
- At the first block line where `window ≥ fade_len`:
  1. apply the rewires (`inputs[port] = steady`, `seen[port] = COPY`; plugin `audio_in`; feedback
     `input`; `out`);
  2. switch to the steady order;
  3. move every ghost exec's state and every ghost lane into `union.ghost_box`;
  4. retire it as `Retired::Ghosts`. Room was reserved in §5.2.
- `Panic` during a window ends the window first, then resets the plan in place (RENDER-10).
- `Hold(true)` waits for the window to end. Idling (RENDER-6a) is off during a window, as during
  today's fade.

### 5.5 Clocks and transport

The engine owns the transport (`Engine.transport`, out of `Renderer`). It is never copied from plan to
plan, so `enter_from` is deleted. `Jump::Enter` stays for a fresh engine's first block and for
`Panic`: seek without chasing, and end nothing.

### 5.6 Retiring, and memory that can't grow

- Every object an activation replaces is in the shell or the ghost box, and the room for both is
  checked before the activation applies. The `mem::forget` fallback stays behind a `debug_assert`,
  and should be unreachable: G7g counts its uses (`Engine.leaked`) and requires 0.
- One `Store` per instance (§9) means dropping a `KernelInstance` frees its memory. No retired
  instance can be pinned by a live one.
- **Batch drops.** The dropper (`drop_retired`, `core/play/src/lib.rs`) drops a shell's instances at
  most 64 at a time, sleeping 1 ms between batches. Thousands of `munmap` calls then can't hold the VM
  map lock long enough to stall the audio thread's page faults. Retired containers expose their
  instances as a `Vec` to drain. G7f measures the contention with and without batching.
- **Counters.** `KernelInstance` increments a static `LIVE_KERNELS` in `new` and decrements it in
  `Drop`. With `PluginHost::live()` and the owner's `live()`, these are what G7g compares with their
  values before the run.

### 5.7 Panic, idle, a trap

- `Panic` resets the plan in place: kernels, plugins (`reset`), rings, voices, schedulers, audio
  players. The next block is `Enter`.
- `can_idle` and `woken` read the plan that plays. The engine never idles during a window or a hold.
- **A trap.** Today the whole renderer goes silent (`broken`) and stops. That stays for the plan. In
  addition, the trapping slot's bit is set in `plan.poison` (an `Arc<[AtomicU64]>` shared with the
  `PlanIndex`). The next prepare treats a poisoned slot as unmatched, so a crashed kernel is never
  kept (F12). A kernel in a voice pool poisons its `poly`.

---

## 6. Lanes and arrangement switches

### 6.1 Schedulers persist, and note ids are engine-wide

- A `NoteScheduler` lives in `Data` for the engine's life, keyed by `LaneAddr`, and moves through
  `LaneMove::Keep`. So a kept consumer keeps receiving its lane's events, Offs included. There is no
  adoption, no `raise_next_id` and no `release_all`. The `released` and `adopted` flags go too.
- **Note ids come from one engine counter** (`NoteIds`, a `u32` skipping 0), passed into each
  scheduler's `run`. Schedulers run in plan lane order, so ids are deterministic offline and live from
  a fresh engine. `poly` doesn't depend on id values, so renders of pure songs keep their bits. A
  plugin whose lane changed can never see an id it has already seen, which removes
  `PluginInstance.next_id`.
- **Replaced consumers are released** (RENDER-26a, bullet 2) as ghosts (§5.3). **A kept consumer whose
  lane changed** (identity note F8): a `poly` can't be kept, because its lane is part of its shape; a
  plugin releases its keys. **Notes the scheduler still holds for a consumer that is gone** get their
  Offs later, which find nothing. That is harmless, and `gates_are_owned` still holds.
- **`Grow`.** A notes edit that outgrows its scheduler's `Caps` is an activation whose data has a
  `Grow` for that lane, not a whole rebuild. This is the only place a scheduler's sounding notes are
  copied. The code is today's `adopt`, renamed `grow_from`, minus the `next_id` handling.

### 6.2 `SwapLane`: notes, control and audio

- **Addressing.** The message is addressed by `(PlanId, lane index)`, which the actor resolves from
  `LaneAddr` against the plan it last sent. The `PlanId` check catches an actor bug, at the cost of one
  compare. A swap addressed to another plan is refused and retired, as today's generation check does.
- **Audio lanes now swap too**, which RENDER-27 needs and today requires a rebuild:
  - The actor renders the changed lane's clips off the audio thread through `audio::Cache`, keyed by
    asset hash, clip fields, tempo and rate, then sends `SwapLane` with `LaneSwap::Audio(Arc<AudioLane>)`.
  - `AudioPlayer::swap` keeps the replaced data as `prev`; the one before that is returned to retire.
    At the next block, the old material fades out from its continuation, reading `prev`, equal-power
    over 5 ms (`Tap` gains a `src: u8`), and the new material fades in at the transport position.
  - This is the same declick as a jump (RENDER-9 item 5). A no-op swap, where every clip render is
    the same `Arc` from the cache, sets no taps and changes no bit.

### 6.3 Arrangement switches (RENDER-27)

A tempo or meter change is an `Activate` that keeps every instance:

- `Plan::prepare` gets an identity continuity (same revision code), so every node is kept and the
  union is empty.
- `Data::prepare` rebuilds every notes lane's `NotesData` (sample positions depend on tempo) and every
  warped audio clip render, all off the audio thread. Each becomes a `Keep { swap: Some(..) }`, plus
  the new tempo map and arrangement.
- At the block line, all of it switches at once (§5.3 step 5): the transport is retimed by beat, each
  scheduler reconciles its sounding notes against the new data (their Offs come from the new data's
  end samples, so RENDER-26a's "recompute the frame of every pending Off from its beat" holds), and
  audio players crossfade to their new renders.
- **No kernel can lose a tail on a tempo change.** Every one is kept, by construction.

A mixed change (code plus tempo, PLANE-2) is the same single `Activate`, with fades where the code
changed.

### 6.4 Assets

A changed asset (`delta.assets`) re-renders the clips that play it, then takes the audio `SwapLane` of
§6.2 for each affected lane. RENDER-26b's "a changed asset takes the path of RENDER-27" is that.

---

## 7. Plugins: persistent instances, `LoadState`, restart

### 7.1 Ownership

- `PluginInstance` stays an `Arc`: lifetime is shared with an export that borrows it (§8), and the
  last drop queues `host.destroy` to the main thread, as today.
- **Processing is exclusive by construction.** Only the plan slot holding the `PluginState` box calls
  `process` or `reset`, and that box is moved, never shared. Ghosts are other instances. An export
  processes only while the engine is held. So "an instance processed twice in a block" can't be built.
  The probe's "steady time processed twice" counter checks it in `no_alloc.rs` and in the with-plugins
  gate.
- `PluginInstance` gains `shape: u64`, `restart: AtomicBool` (kept from today) and
  `reassert: AtomicBool`. It loses `key`, `held_cap`, `next_id` and `hold()`.

### 7.2 `LoadState` (RENDER-26b, HOME-4, HOME-6)

- It is a main-thread host call, not an engine message. The actor finds the instance by state path in
  `PlanIndex.plugins`, reads the blob, and calls `host.load_state(h, blob)`. Then it sets
  `inst.reassert`. At the instance's next `process`, the node swaps `reassert` to false and clears
  `sent`, so every code-owned parameter is sent again in that call. HOME-4 then holds: code beats the
  blob. The window where the blob's value plays is under one block.
- **A host fix:** `InProcess::load_state` (`core/plugins/src/inprocess.rs:275`) skips a blob whose hash
  equals `last_saved`, but never updates `last_saved` when it loads one. So save(S), load(C), load(S)
  wrongly skips the last load. The export depends on exactly that sequence (§8). Rename the field
  `holds` ("the state it is known to hold") and set it on every successful load and every save. The
  doc of `PluginHost::load_state` changes to match (HOME-6).
- **Latency.** After a load, the actor queues `host.latency(h)`. A nonzero latency raises the Warning
  `latency.unsupported` naming the plugin, and it plays uncompensated. RENDER-21's refusal at check
  time stays. RENDER-17's live re-plan waits for compensation.
- **A plugin with no saved state** never loads anything (FMT-17).

### 7.3 Notes into plugins

`keys` is instance state and moves with the instance. Its capacity is the most notes any plan fed it
(`held_room`, wiring), grown by copying at activation when a new plan needs more (§5.3). `ended`
(owed chokes after a panic) moves too. `release` is wiring: notes to let go at the first call (a lane
change, or a ghost).

### 7.4 Restart (`HostEvent::Restart`)

The actor marks the instance and, at once, prepares an activation of the revision that plays with
identity continuity. The marked instance doesn't match, so a fresh one is created *from
`save_state(old)`*, which keeps its patch and in-memory state. It fades in while the old one rings out
as a ghost for 5 ms. Today's behaviour is weaker: it waits for the next rebuild and loads the blob from
disk. A real deactivate/activate of the same instance would need a per-instance hold. It is deferred to
plugin windows, whose trigger is already listed.

---

## 8. Export: pause, reuse, resume (RENDER-1)

### 8.1 Hold

`Msg::Hold(true)`, applied at once:

- The engine keeps processing while it fades its output to 0 over `fade_len` frames, then stops
  processing the plan altogether: no kernel, no plugin, no scheduler.
- It sets `held = true` in the player's `Shared`, published at the end of that callback. When the
  export thread sees `held`, the audio thread has returned from the last block that called any plugin.
  It won't call one again until it reads `Hold(false)`, which only the export sends. Structural
  messages wait (`ready` requires not held), and `Panic` is deferred.
- `Hold(false)` resumes processing with a 5 ms fade-in from where it was. That is a tape pause: what
  was sounding continues, except plugin voices, which the export reset. At resume each plugin node
  clears `keys` (the plugin forgot them; later Offs find nothing) and `sent` (parameters are re-sent).
- The transport doesn't move while held. Commands sent meanwhile apply to it and take effect at the
  resume. The server's state says `paused: "exporting"`, which the app shows (RENDER-1's "the UI says
  so").

### 8.2 The sequence (`crates/server/src/export.rs`, `start`)

```
pure song (no FOp::Plugin in order): as today, a Renderer on its own thread, beside playback.

otherwise:
  actor: exporting = true         settles stop sending to the engine; they're remembered
  player.hold(true); wait up to 2 s for player.held()
      not held (device stalled): fall back to fresh instances, with a Notice saying so
  match accepted revision's plugin nodes to live instances: continues(plays.revision, accepted)
  for each borrowed instance, on the plugin main thread:
      before[h] = save_state(h)                  // the state it has now, to give back
      deactivate(h)
      load_state(h, committed blob)              // when it has one; `holds` makes this exact
      set_render_mode(h, Offline)
      activate(h, rate, BLOCK)                   // latency measured (RENDER-17)
  Renderer for the accepted revision: kernels fresh; plugins from the borrowed instances
      (`lend`), fresh for nodes with no live instance
  render thread: reset(h) for each borrowed instance before its first process (takes the audio role)
  render to the WAV (as today: write_new / write_for)
  finally (success, error, stop, or a panic caught): on the main thread, for each borrowed:
      deactivate(h); set_render_mode(h, Realtime); load_state(h, before[h]); activate(h, rate, BLOCK)
      inst.reassert = true
  player.hold(false); actor: exporting = false; settle once
```

- This changes the contract's call order. RENDER-1 lists `deactivate, reset, load_state,
  set_render_mode(Offline), activate`. But `reset` is an audio-thread call that CLAP allows only while
  the instance is active, and the host's `reset` does nothing on an inactive instance. So `reset`
  moves to the render thread, after `activate` and before the first `process`. That reset is what
  makes the export start exact. The probe test (below) proves it.
- **State intact**: `save_state` returns the same bytes before and after the export, which G7e checks.
- **What the actor does meanwhile.** Accepting, compiling and every client request carry on. Only
  `settle` skips the engine while `exporting`. Lease updates still write cells; the engine applies them
  at the resume.
- **Failures.**
  - A failed host call on one instance makes that node fresh for the export, and records it in the
    export's `problems`.
  - The `finally` block always runs: it is a guard object dropped on every path.
  - If the restore fails for an instance (its `load_state(before)` errors), that instance is marked
    restart-flagged, so the next settle replaces it with a fresh one built from `before`.

### 8.3 Range renders and the CLI

- `m2 render`, `m2 bounce` and agent range renders keep creating their own instances: the CLI has no
  live engine.
- An app-side range render of an impure song would use the same hold and borrow. None exists yet.

---

## 9. The Store layout, measured at 6,176 instances (RENDER-32)

### 9.1 Default: one Store per instance

`wasmtime::Store` frees nothing until it is dropped (wasmtime 49.0.2, `store.rs`). So:

- **One long-lived arena Store** leaks every replaced instance's memory for the session: F8.
- **One Store per prepare batch** (the contract notes' fallback): the open's batch holds about 6,000
  instances. Replacing any of them leaks it until all 6,000 are gone, which may never happen. That is a
  bounded leak, but of about the song's size.
- **One Store per instance** frees exactly what retires. It also lets instances from two `PackageSet`s
  (two wasmtime `Engine`s) coexist, which G7d needs.

`KernelInstance` owns its `Store<StoreLimits>`, with today's limits (`KERNEL_MEMORY`). `Graph::process`
no longer threads a `&mut Store` around: each kernel calls into its own. That also simplifies every
signature in `lib.rs`.

### 9.2 The `store` gate (G7f): what is measured

`m2-gates store --reference 50`, a new gate in `crates/gates/src/store.rs`. It uses no server and no
device. It compiles p50, counts the instances, and builds the kernel population directly with wasmtime
under each layout. It doesn't go through the runtime's `Graph`, so the runtime needn't support several
layouts.

1. **The count.** Top-level kernel nodes, plus voices × kernel nodes for each `poly`. It prints the
   count (the survey estimates about 6,060) and the contract's 6,176 is corrected from it.
2. **The layouts:**
   - L1: one Store per instance, default reservation (4 GiB plus guards);
   - L2: one Store for all, today's layout, as the baseline;
   - L3: one Store per instance with `memory_reservation(KERNEL_MEMORY)` and
     `memory_reservation_for_growth(0)`;
   - L4: one Store per instance with the pooling allocator (feature `pooling-allocator`, sized for
     2 × 6,176 instances);
   - for reference only, L5: one Store per track.
3. **For each layout:**
   - build time per instance, split into `Store::new`, `Instance::new`, the typed-func lookups,
     `k_init` and the prewarm `k_reset`;
   - process memory: `task_info` phys footprint before and after;
   - address space reserved: the sum over `mach_vm_region`, compared with the process limit;
   - block time: every instance's `k_process(128)` in p50's order, 30 s of blocks with inputs filled
     from a fixed-seed noise generator; mean, p99 and max, as a share of the 2.67 ms budget;
   - drop time for all instances on another thread, and the p99 block time of a concurrent
     audio-like thread during that drop, with and without 64-instance batches.
4. **The decision rule, stated now.** L1, unless one of these holds:
   - its p99 block time is more than 5% above L2's;
   - its phys footprint is more than 50 MB above L2's;
   - its peak reservation at twice p50's count is more than 25% of the process's address-space limit;
   - drop contention raises the audio-like thread's p99 by more than 10% even with batches.

   If one does, try L3 (bounds checks cost DSP, which the gate shows), then L4. Never L2 or a shared
   batch Store: those leak. The run and the choice go into RENDER-32's Today line.

### 9.3 Instantiation cost

Prepare creates only the new instances. A whole replaced track is 156 at most (a pluck pool), so the
200 ms gate doesn't depend on the per-instance build cost. Opening a project does: 6,000 instances at
the L1 cost. G7f prints the open time too. It is not gated, but it is watched.

---

## 10. The gates, and how each is measured

Rig changes first, in `crates/gates/src/engine.rs` (the only file that calls the engine):

- `Song` carries its `Arc<Revision>`, so the rig can call `continues` exactly as the server does.
- `Rig::prepare` builds `Next` through `Plan::prepare` and `Data::prepare`.
- `--no-carry` becomes `--no-arena`: an empty continuity, so everything is fresh and the output fades
  whole. That is the negative reference for every arena gate.
- `Probe` gains the last activation's `kept`, `created`, `ghosts`, `fades` and `refused` counts, plus
  `leaked`.

| Gate | How | Pass | Negative control |
|---|---|---|---|
| **G7a** a structural edit at 3:00 in p50 is audible in ≤ 200 ms after the compile | `m2-gates --server --reference 50 --at 180 add-effect`. `Opts.at` is new; `open()` seeks there instead of 1:00. Ten edits through `server::add_effect`, alternately adding `delay` to `t02` and removing it. `heard_ms` is counted from `compiled_ns`, as today. The report splits each edit into continuity, prepare (with the number of fresh instances), send and the wait to the block | every edit's `heard` ≤ 200 ms; no jump back (`Sampler`); fresh instances ≤ 3 per edit | — (a timing gate) |
| **G7b** tails of untouched tracks intact | New engine-rig gate `tails` on p50. Deck A: at 3:00, activate p50 with `t02`'s instrument wrapped in `vca(…, 1)` (`std:scale`, an exact multiply by 1.0), written as a file save of `tracks/t02.ts` (the agent's path). Deck B: the same, never activated. Both are filled with the same buffers. Repeat with the wrap made by the app's `add_effect` Tx | identical before the applied frame; within the fade window, `|A − B| ≤ 1e-6`; from the applied frame plus `fade_len` to 20 s later, `A == B` bit for bit; counters: `kept` = every old slot, `created` = 1, `ghosts` = 0, `fades` = 1, `resets` unchanged; the activation's callback takes at most 1.25× the median callback's wall time | `--no-arena` must fail the bit-for-bit check by more than −60 dBFS. A gate that passes on HEAD measures nothing |
| **G7b′** the audible version | Deck A gets `delay` on `t02`; deck B nothing | A differs from B after the applied frame by more than −60 dBFS (it is heard); before it they are equal; G6's step bound holds across the fade | — |
| **G7c** live without edits is bit-identical to offline | `core/play/tests/bit_identity.rs` as is, now on `Plan`. Add `a_no_op_activation_changes_no_bit`: first-song and p16, at 5 s activate a plan prepared from the same revision against the playing one. Add `an_identity_arrangement_activation_changes_no_bit`. Add `live_is_the_export_on_all_of_p50` (`--ignored`, recorded) | equal to offline bit for bit, fade window included | the same test with a forced fresh poly must differ (the test's own check) |
| **G7d** a rebuilt kernel module starts from init | Runtime test `a_rebuilt_kernel_module_starts_from_init` in `core/runtime/src/tests/arena.rs`. PackageSet A is std. B is a copy whose `std.wasm` has one extra custom section (valid, the same code, a new SHA-256). A delay with feedback plays an impulse. Mid-echo, prepare the same `Flat` against B and activate | arena counters: no kernel kept; after the window the echo is gone (B's delay is empty), whereas never activating keeps it | control: C, loaded from identical bytes (a new `Module`, the same hash), keeps every instance, and the output is bit-identical to never activating. A second test feeds the audio thread a move with a wrong shape hash: refused, `refused` = 1, the old plan plays on |
| **G7e** the export of with-plugins pauses, reuses, resumes with state intact | (1) Server gate `m2-gates --server export`, skipped without Surge XT and Airwindows on `CLAP_PATH`. Play from 0:02, export through `m2_server::export::start`. (2) CI test `an_export_borrows_and_gives_back_the_live_instances` in `crates/server`, with the probe (`clap:m2.probe`, saved state, gain set by code so the in-memory state differs from the blob) | *Pauses:* `held` before the first host call, `paused: "exporting"` published. *Reuses:* `created()` unchanged; the plan's handles before = after. *State intact:* `save_state` bytes before = after. *Resumes:* playing at the same transport frame within two buffers. Probe only: the main-thread call log is save_state, deactivate, load_state, set_render_mode(Offline), activate, then deactivate, set_render_mode(Realtime), load_state, activate; no audio-thread `process` between the hold and the resume; the reused export equals the export with fresh instances bit for bit; `steady time processed twice` = 0 | the probe test with the hold removed (test-only flag) must see audio-thread process calls inside the window |
| **G7f** the Store cost at 6,176, layout chosen | `m2-gates store --reference 50` (§9.2) | prints the count and the table; the decision follows the stated rule | L2's leak: build, replace 10% of instances 20 times, and L2's footprint grows while L1's doesn't |
| **G7g** no memory growth (new; F8) | `m2-gates --reference 16 churn`: 200 structural edits while playing (add and remove an effect, alternately, on rotating tracks; plus 20 tempo changes). CI version: first-song, 50 edits, in `core/play/tests/live.rs` | after draining: `LIVE_KERNELS` and the plugin `live()` equal their values before; `Engine.leaked == 0`; phys footprint ≤ +20 MB (recorded run) | a test-only Store-per-plan-batch build must fail the count |

**Gates that must keep passing:**

- step 3's G1 to G7, G10, G11, G8 `previews` and G9 `drag`;
- with-plugins, where "carried instances" checks become "kept by the arena" checks with the same
  meaning (`same_instances` in `crates/gates/src/plugins.rs`);
- `core/session/tests/renders.json` (offline renders byte-identical to before step 7);
- `no_alloc.rs`, `no_blocking.rs`, and the step 6 Tx gates (with their names updated).

---

## 11. Tests, by crate (each names its rule)

**`core/script`**

- `every_call_has_an_open_paren`: `Site.open` on every fixture and every std file. It points at a
  `(`, and lies within `start..end`.

**`core/session/src/text.rs`**

- `token_hunks_are_an_edit_script` (property): the hunks apply old to new; they are sorted, disjoint
  and on character boundaries; carried bytes are equal.
- `two_literals_on_adjacent_lines_are_two_hunks` (identity note F1).
- `a_reformat_changes_no_token`.

**`core/session/src/tx/delta_tests.rs`**, all through `delta_between`:

- `a_wrap_by_a_file_write_keeps_the_wrapped_names`, on p50's `t02` text, the nested
  `reverb(pad(…))`, and the unwrap (F2).
- `adjacent_value_edits_are_values`.
- `a_component_swap_is_new_and_an_alias_is_not` (PKG-9).
- `names_ignore_everything_above_a_key_frame` (ID-7).
- `old_side_names_are_own_offsets` (the cache's premise).
- `delta_names_are_unique_on_every_fixture`, kept.
- The step 6 two-process hash test, kept, with its expected names updated.

**`core/session/src/tx/continuity_tests.rs`**

- `an_insertion_above_continues_everything` (step 6's gate edit).
- `a_label_rename_continues_everything` (ID-10).
- `a_loop_that_grows_resets_its_occurrences` and `a_keyed_loop_keeps_through_reorder_and_growth`
  (ID-14).
- `a_moved_keyed_plugin_continues` and `adding_a_plugin_key_continues_its_instance` (ID-8).
- `a_rewired_notes_input_keeps_the_name` (the runtime decides what that means).

**`core/runtime/src/tests/arena.rs`** (new)

- `the_same_revision_keeps_every_instance_and_fades_nothing`
- `a_wrapped_instrument_keeps_its_voices_and_fades_one_edge` (poly `born` and notes unchanged; one fade)
- `a_removed_effect_rings_as_a_ghost_then_retires` (the ghost runs `fade_len` frames, then it is retired)
- `an_effect_reorder_replaces_the_cycle` (cycle rule; the rest kept)
- `a_poly_whose_voice_changed_is_replaced_and_released` (no gate open after the window)
- `a_poly_moved_to_another_lane_is_replaced` (identity note F8) and
  `a_plugin_moved_to_another_lane_lets_go_of_its_keys`
- `a_kept_plugin_is_processed_once_per_block` (probe)
- `feedback_rings_survive_an_activation` (no 128-sample hole)
- `a_committed_smooth_value_ramps_and_a_plain_one_steps` (RENDER-22a through activation)
- `a_kept_voice_pool_gets_only_its_changed_cells` (a write counter)
- `a_lease_value_set_before_sending_plays_from_the_first_block`
- `an_activation_against_another_plan_is_refused`, `a_wrong_shape_move_is_refused` and
  `a_repeated_source_is_refused`
- `a_trapped_kernel_is_never_kept` (F12)
- `a_rebuilt_kernel_module_starts_from_init`, with its identical-bytes control (G7d)
- `a_tempo_change_keeps_every_kernel_and_ends_notes_at_their_beats` (RENDER-27, RENDER-26a)
- `an_outgrown_lane_grows_without_cutting_notes`
- `note_ids_are_engine_unique` and `offline_renders_keep_their_bits` (against `renders.json`)
- `panic_during_a_window_ends_it_then_resets`

**`core/runtime/src/sched.rs` and `audio.rs`**

- `a_scheduler_kept_across_plans_keeps_its_notes` (replaces the adoption tests).
- `an_audio_swap_crossfades_without_a_step` (G6-style bound).
- `a_no_op_audio_swap_changes_no_bit`.

**`core/plugins`**

- `load_then_restore_is_never_skipped` (save S, load C, load S: S loads).
- `a_load_of_the_held_state_is_skipped` (HOME-6).

**`core/play`**

- `no_alloc.rs`, extended. Under the trap: an activation with kept kernels, a kept poly, a kept
  plugin, fresh nodes, a ghost poly and a ghost plugin, three fades, a lane `Keep` with a swap, a
  `Grow`, an audio swap with its crossfade, the window's end and the ghosts' retirement, a refused
  activation, `Hold(true)` then `Hold(false)`, and a deferred panic. All this with cell writes from
  another thread throughout, as today.
- `bit_identity.rs`: G7c's additions.
- `live.rs`: `random_edits_and_jumps_never_strand_a_gate` now includes structural edits (random
  wraps, unwraps and replacements) and asserts `gates_are_owned` after every window. Plus G7g's CI
  version.

**`crates/server`**

- `an_export_borrows_and_gives_back_the_live_instances` (G7e, probe).
- `settles_wait_for_an_export_then_apply_once`.
- `loading_is_published_while_a_plugin_is_created`.
- `a_restart_request_replaces_the_instance_from_its_saved_state`.
- `a_failed_build_then_a_lane_edit_rebuilds`, kept.
- The lease tests, kept: re-aimed at every activation, as today.

---

## 12. Build order: four slices, each landing green

Each slice keeps the workspace building and every existing test and gate passing.

**Slice 1: groundwork (about 1 week). Nothing audible changes.**

- `core/runtime`: `KernelInstance` with its own `Store`; `Graph::process` without the store
  parameter; `LIVE_KERNELS`; the shape hash; `PackageSet::module_sha(kernel)`, from `Package.files`.
  Today's `Renderer` keeps working, with one Store per instance.
- `crates/gates`: the `store` gate (G7f). Run it and record the decision. If L1 fails, switch the
  engine configuration in `packages::engine()` here, before anything else depends on it.
- `core/plugins`: the `holds` fix, with its two tests.
- `core/script` and `core/session`: `Site.open`; the token refinement; ComponentId, the `key()` pin and
  `counts` in delta names; `continues`; their tests. Step 6's name expectations are updated.
- **Coordination.** The data-plane workflow is editing `core/session` and `crates/server` now. Rebase
  on it first; `text.rs` and `tx/delta.rs` are the likely conflicts.

**Slice 2: the arena (about 2 to 3 weeks). This is the core change.**

- `core/runtime`:
  - `plan.rs`: `Plan`, `Data`, `PlanIndex`, `prepare` with keeps, ghosts, fades, the union order, the
    cycle rule and the poly rule;
  - `engine.rs` rewritten: one plan, `Activate` with checks and moves, the window, ghost retirement,
    the transport in the engine, `SwapLane` by `(PlanId, lane)`;
  - `sched.rs`: persistent schedulers, engine-wide ids, `grow_from`;
  - `audio.rs`: `AudioPlayer` in the lane;
  - `Renderer` as the offline owner;
  - delete `plugins.rs`'s carry machinery.
- `core/play`: `Player::activate(Next)`, `swap_lane(PlanId, u32, LaneSwap)`; `Applied.refused`; the
  dropper in batches; `no_alloc.rs` and `bit_identity.rs` extended.
- `crates/server/src/graph/actor.rs`:
  - `Plays { epoch, index: PlanIndex, revision, cell_slots, cells, controls }`;
  - `way`: empty delta → nothing; lanes only → `SwapLane`; everything else → `Activate` with
    `continues`;
  - delete `same_layout`, `value_writes`, `Reach.cells`, `adopt`'s layout path, `Open.carry`, and the
    carry in `Way`;
  - lease re-aiming unchanged.
- `crates/editor/src/render.rs`: `Render` on the new `Renderer` (the same API).
- `crates/gates`: the rig on the arena, `--no-arena`, G7a (`--at`), G7b and G7b′, G7d's optional gate
  form, and the with-plugins checks renamed.

**Slice 3: the data planes and plugin state (about 1 week).**

- Arrangement changes as `Activate` (retime; rebuilt `NotesData` and clip renders).
- Audio-lane `SwapLane` with prepared renders and the `prev` crossfade; assets through it.
- `LoadState` as a main-thread call with `reassert`; `latency.unsupported`.
- The `loading` status.
- Restart from saved state.
- Poison on trap.
- Their tests.

**Slice 4: export reuse and the contract (about 1 week).**

- `Hold` in the engine and the player; the export sequence in `crates/server/src/export.rs`; the
  actor's `exporting`; G7e (the probe test and the with-plugins gate); G7g.
- The contract edits (section 14) and the stale Today lines `understand-contract.md` §4 lists.
- The debug-build allocation trap in `apps/music2-desktop` (RENDER-28: "Debug builds … run the audio
  thread under an allocation and lock trap"). This is the counting allocator from `no_alloc.rs`, made
  a module of `m2-play`. It logs once a second while the count is nonzero, rather than panicking in
  the callback.
- Optional cleanups (§13, last list).

That is 5 to 6 weeks in all, within the order of work's 4 to 6.

---

## 13. What is deleted or unified

**`core/runtime/src/plugins.rs`**

- Delete: `CarryKey`, `Carry` (`none`, `of`, `source`, `handles`, `restart`), `InstanceSource`, `Fresh`,
  `carry_from`, `lend`.
- `PluginInstance` loses `key`, `held_cap`, `next_id` and `hold()`. It keeps `restart` and its
  destroy-on-drop. The file shrinks to `PluginInstance` alone.

**`core/runtime/src/lib.rs`**

- Delete `Renderer::{carry_from, render_block_lent, enter_from, generation, lane_slots}` and the
  `release_all` parameter of `render_block`.
- Delete `PluginNode.lent` and `PluginView.lent`, `Builder.{source, ordinals}`, and the dead
  plugin-in-voice branch of `Builder::plugin` (`in_voice`, `ordinal = u32::MAX`). The checker already
  refuses plugins in `poly`.
- Delete the static `GENERATION` (replaced by `PlanId`).
- Delete the `store` parameter threaded through `Graph::process`, `reset`, `prewarm`, `active`,
  `PolyNode::process` and `KernelNode::{process, reset}`.

**`core/runtime/src/engine.rs`**

Delete the two-renderer `Fade`, `fading()`, activation coalescing, and the whole-output linear mix.

**`core/runtime/src/sched.rs`**

- Delete `release_all` and `released`, `raise_next_id`, the per-scheduler `next_id` (replaced by
  engine-wide ids), and `adopted`.
- `adopt` becomes `grow_from`, with one caller.

**`core/runtime/src/audio.rs`**

Delete the "nothing is faded at a renderer's first block" special case. `AudioNode` becomes the
`AudioPlayer` inside the lane entry.

**`crates/server/src/graph/actor.rs`**

- Delete `same_layout` and `value_writes`.
- Delete `Reach.cells` and the cell-write branch of `Way::Reach`.
- Delete `Open::adopt`'s `same_layout` and `CellMap` re-key. Adopting remains for lane swaps.
- Delete `Open.carry`, `Carry::of` and `carry.restart` (restart becomes §7.4), and the `Carry`
  carried in `Way::{Activate, Replace, Load}`.
- The "values without `same_layout` → new renderer" row of the step 6 design's 5.5 disappears: every
  code change keeps what it can.

**`crates/gates`**

- `--no-carry` becomes `--no-arena`.
- The rig's `Carry` and `carry_epoch` go.

**Unified**

- *Lanes:* `SwapLane` for notes, control and audio lanes, with one addressing scheme, `(PlanId,
  lane)`.
- *Activations:* code, arrangement, values, a lane outgrowing its scheduler, and a plugin restart
  all go through one `Activate`.
- *Crossfades:* one fade mechanism for activations (the edge fades). `LiveFade` in `core/play` stays
  only for what replaces a whole engine: opening a project, a rate change, closing.
- *Plugin parameters:* "what a plugin was told" (`sent`) is one table, used by HOME-4, by `LoadState`'s
  reassert and by the export's resume.

**Optional, outside the engine** (from `understand-identity.md` §5; do them in slice 4 or later)

- Replace `crates/editor/src/controls.rs`'s `carry_text`, `carry_ends` and `carry_lines` with
  `m2_session::text::Edits::between(old, new).carry(range)`. Then leases, cables and control ids carry
  by the same correspondence as Tx ranges and undo.
- Delete `m2_ir::control_id`, which only a test uses.

**Contract text that goes**

- HOST-5's step 3 matching sentence.
- RENDER-25a's "Step 3 … crossfades the whole output".
- `SetArrangement` and `LoadState` in the engine shape and in RENDER-28's list.
- ID-13's numbered `SiteId`, and its rule 4, which moves to Deferred.
- Step 3b's pending clarification C7 (adoption) is withdrawn: nothing adopts any more.

---

## 14. Proposed contract edits

| Rule | Now | Proposed | Why it is simpler or safer |
|---|---|---|---|
| ID-13 | "the session MUST give each site a `SiteId(u32)` when it accepts a code revision … 1–4" | "A site's identity across two revisions is its file and the point of its argument list's `(`, in the comparison's reference coordinates (WRITE-8). Nothing numbers sites. A Tx is judged in its own edits' coordinates; every other comparison, the engine's included, uses a line diff whose hunks a token-level Myers diff refines. A `key()` frame is named by its key, and cuts the positional chain above it. An ambiguous match gets a fresh identity, and its state resets." Rule 4 moves to the Deferred row "Keyed iteration helpers and better structural matching". | A counter is history (failed builds, undo, reopen) and would make `DeltaHash` differ between processes. The engine only compares what plays with what is accepted |
| ID-7 | "pins SiteIds from step 7" | "is named by its key in every identity below it (ID-13)" | says what is built |
| ID-14 | as is | add: "Applied when the arena matches instances; a `key()` frame has no occurrence." | — |
| ID-15 | "RuntimeId = InstancePath / (the site of the `kernel()` or `plugin()` call …)" | "… of the `kernel()`, `plugin()`, `poly()` or `feedback()` call … Segments are `key:<k>` or `<ComponentId>@<file>:<point>#<occurrence>`. A keyed plugin that no RuntimeId matches is matched by its state path. A `poly` is kept whole, with its voices in their slots, or replaced whole." | poly and feedback hold state too; the state-path pass keeps HOST-5's property |
| WRITE-8 | "… each frame a call site given as its byte range … From step 7, SiteIds replace the mapped ranges." | "… each frame its ComponentId and its call's point (ID-13) …". Delete the last sentence. | the points are the SiteIds |
| RENDER-1 | "deactivate, reset, load_state of the committed blob, set_render_mode(Offline), activate. Afterwards … given back the state they had." | "The engine is held (no thread processes any instance, proved before any call). Each reused instance's state is saved, then deactivate, load_state of the committed blob, set_render_mode(Offline), activate, and reset on the render thread before its first process. Afterwards deactivate, set_render_mode(Realtime), load_state of the saved state, activate, and the engine resumes from where it held." | CLAP's `reset` needs an active instance; the hold makes exclusivity provable |
| RENDER-10 | as is | add: "A plan activation never resets a kept instance." | states the arena's guarantee |
| RENDER-23 | Today: "cells live and die with their renderer until the arena keeps them across plans" | "Cells stay per plan. At activation, a cell whose constant continues one of the old plan's and whose value differs is applied as a `SetCell` would be (RENDER-22a)." | committed values sound like `SetCell`s without the server's layout matching |
| RENDER-25 | "Activation keeps every instance whose RuntimeId and shape match. The shape is …" | add: "The match is computed off the audio thread, between the plan that plays and the new one, and sent as moves relative to the plan it was prepared against. The audio thread checks that plan's id and each move's shape before moving anything, and refuses the whole activation on a mismatch. The shape of a kernel is its id, its package's wasm SHA-256, its channels, the ABI version and the rate; of a plugin, its id, version and rate; of a `poly`, its voices, channels, lane and voice-graph structure; of a feedback, its channels." Fix the stale Today text about the retired queue. | makes F6 and F11 impossible, not just unlikely |
| RENDER-25a | "… old and new instances run together for one fade window, and the fade is applied at least at every module output whose subgraph changed … Step 3 … crossfades the whole output." | "For one fade window of 5 ms the engine plays the union of the old and new graphs: an instance they share is processed once, a replaced node runs on with its notes released, and every input whose source changed is crossfaded from the old source to the new. Nothing else is faded. An edit whose union would have a cycle (a reorder of kept nodes) replaces the nodes on the cycle." | no instance is processed twice; the CPU cost is the replaced nodes, not the song; lending goes |
| RENDER-26 | "`SwapLane` MUST replace one lane's data in the running nodes …" | add: "Audio lanes swap too, with their clip renders prepared off the audio thread; the old material crossfades out over 5 ms." Addressed by plan id and lane index. | one data path for every lane kind |
| RENDER-26a | "An activation that replaces a note consumer first sends an Off …"; Today: adoption, C7 pending | "Each notes lane's scheduler persists for the engine's life, keyed by its `LaneAddr`, and note ids are unique across the engine. An activation that replaces a note consumer, or keeps a plugin whose notes now come from another lane, first sends an Off for every note sounding into it; a `poly` whose lane changed is replaced." Withdraw C7. | stuck notes from rewiring become impossible; adoption goes |
| RENDER-26b | "MUST reach the running instance as `LoadState` … If the latency changed, a plan-only activation re-plans …" | "… as a load on the plugin main thread into the instance that plays, skipped when its hash equals the state the instance holds (last saved or last loaded); its code-owned parameters are sent again at its next block (HOME-4). Until compensation ships, a latency that becomes nonzero raises `latency.unsupported` and plays uncompensated." | no engine message; HOME-4 holds by construction |
| RENDER-27 | "… then switch them together at one block boundary" | add: "An arrangement change is an activation that keeps every instance." | one path; a tempo change can't cut a tail |
| RENDER-28 | message list `(Activate, SwapLane, SetArrangement, LoadState, Transport)`; "the sender waits … or coalesces to the newest plan" | list `(Activate, SwapLane, Transport, Panic, Hold)`. "Activations are never coalesced: each applies after the previous one's fade window, against the plan it names. A sender gives up after 500 ms, reports the player stalled, and the change is prepared again for the next player." Today: debug app builds count audio-thread allocations. | moves are relative to a base, so coalescing is unsafe; the 500 ms bound is what's built |
| RENDER-32 | "one `Store` per instance, measured … If that costs too much, one `Store` per partition." | "One `Store` per instance (G7f's run, recorded here). If that costs too much: the pooling allocator, or a smaller memory reservation. Never a Store shared across plans, which would keep every replaced instance's memory." | the partition fallback leaks, and partitions don't exist until 8a |
| Engine shape | `Engine { arena: Map<RuntimeId, Slot>, plan, data, cells, … }`, `Msg { …, SetArrangement, LoadState(RuntimeId, …), … }` | `Plan { nodes, order, buffers, cells, slots, latency }`, `Data { arrangement, tempo, lanes: by LaneAddr }`, `Engine { plan, data, transport, engine_clock }`, where the arena is the instances in `plan`'s slots; `Msg { Activate(base, Plan, Data), SwapLane(PlanId, lane, LaneData), Transport, Panic, Hold(bool) }` | what is built |
| HOST-5 | "From step 3 … ordinal and state hash. From step 7 … by RuntimeId." | "Plugin instances persist in the arena by RuntimeId, and a keyed one by its state path (ID-15). A changed state is loaded into the instance (RENDER-26b); a restart makes a new instance from the old one's saved state." | — |
| `PluginHost::load_state` doc | "skipped when its hash equals the state the instance last saved" | "… equals the state the instance holds: the one it last saved or last loaded" | fixes the export's restore |
| DIAG-3 | "from step 7 the notice also compares SiteIds" | "… compares the `(` point of each `track()` call (ID-13), which needs the module's site" or keep deferred | — |
| Deferred | "Sending deltas to clients … With step 7, or when UI update latency is measured over budget." | trigger: "UI update latency measured over budget" | it's a UI concern, not the engine's |

---

## 15. Risks, and points for the owner

**Clarifications to confirm** (numbered for review):

- **C1, ID-13.** A SiteId is a point, not a counter. Everything else in section 2 follows from this.
- **C2, RENDER-25a.** Fades go on every changed input, with replaced nodes run as ghosts, instead of
  fading at module outputs with the old subgraph running whole.
- **C3, the poly rule.** A voice pool is kept whole or replaced whole. An edit inside an instrument's
  voice graph cuts that track's sounding notes, as today.
- **C4, the cycle rule.** Reordering effects resets the reordered effects' tails.
- **C5, export.** Is the hold a tape pause (frozen, then resumes in place) or a stop? A stop would end
  every note at the hold and need the user to press play.
- **C6, committed values.** A committed value always activates (keeping everything), so the server's
  separate cell-write path goes. A lease's value is heard at once, as today; the commit is heard at the
  activation, a few ms later.
- **C7, engine-wide note ids.** Plugin songs' note ids differ from today's. Pure renders don't change.

**Risks**

- **Myers alignment of repeated tokens**, such as many identical `kernel(` calls with one deleted from
  the middle, can hand a neighbour's state over. It is bounded by shape and by ID-14 counts, and it
  can't click (§4.3).
- **The activation block's cost.** It includes the top-level kernels' const rewrites and the moved
  voice pools' cell compares. G7b measures it. If it exceeds 1.25× the median, first move the rewrite
  of a continued cell to a compare-then-write, as voices already do.
- **The cycle rule** is new code with no precedent here. It is covered by `an_effect_reorder_*` and by
  the random structural edits in `live.rs`.
- **Address space.** At L1, p50 reserves about 25 TiB, plus the fresh instances during prepare. A
  99-track song is about twice that. G7f reports the margin. L3 and L4 are the ways out.
- **`munmap` contention** with the audio thread. Batched drops are designed for it, and G7f measures
  it.
- **Plugins that misbehave** under a deactivate/activate cycle or a `load_state` while active. The
  with-plugins gate covers Surge XT and Airwindows only.
- **Old-side name cost.** Caching on `Code` assumes the other workflow's faster Tx path keeps `Code`
  shared between revisions that only change data. Check this when rebasing.
- **Package TypeScript edited during a session** (a std developer's case): package files aren't in
  `coords`, so a moved package site renames its nodes and resets them. That is conservative; it stays.
- **A device stall during an export** falls back to fresh instances, with double the plugin memory for
  that export, and says so.
