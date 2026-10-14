# Step 7 design: the engine

Against `docs/architecture.md` (normative) at HEAD `cbf5eb9`. This is the merged design. Its base is
`design-simple.md`, with the parts of `design-robust.md` that close gaps a producer would hear. Both
designs build on the three survey notes in this folder (`understand-runtime.md`,
`understand-identity.md`, `understand-contract.md`). Nothing was built or run, and no cargo command was
used. Files the data-plane workflow is editing were read as HEAD has them. Every cost marked
*estimate* is unmeasured; G7f and the slice measurements replace it.

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

---

## 0. Judgement: the base, and what was grafted

### 0.1 The two designs against the four criteria

| Criterion | design-simple | design-robust |
|---|---|---|
| **Meets every gate** | Yes, with one defect: reordering two kept effects makes the 5 ms window's graph cyclic, and its prepare has no rule for that, so `comp(eq(x))` → `eq(comp(x))` can't be prepared. Its G7b taps every untouched track and demands bit identity. Its fade is exact when old and new are equal, so a silent edit is bit-identical through the window. | Yes. It has a cycle rule. Its G7b checks only the whole mix, and allows 1e-6 of difference inside the window. |
| **Simplicity** | Fewer concepts: the engine keeps its four messages, `Renderer` keeps its name and API, and there is no `Next`, `Data` or `PlanIndex` split and no engine-owned transport. But it adds an audio-node box kind, a free-running ghost audio node and `PlanInfo.audio`. It also turns audio-lane edits into activations, against the planes table, and keeps the server's value fast path. | More concepts: `PlanId`, `Next`, a separate `Data`, `PlanIndex`, per-slot poison bits, `Hold`, a union order, a cycle rule, batched drops, a seventh gate. But audio lanes are lane state, swapped by `SwapLane` as the contract already says, and the value fast path is deleted. |
| **A producer never hears a regression** | Three gaps. (1) At an activation, a continued cell whose value changed steps, even when its control is declared smooth: a gain jump clicks where today's whole-output crossfade hid it. (2) A kernel that traps, or a filter that turns NaN, keeps its state across every later edit, so the song stays silent until that one node is replaced; today any edit heals it. (3) A ghost that reads a control lane removed in the same edit outputs the lane's default, a step at the start of its fade. It handles a rewired instrument well: kept, its notes released, so release tails ring. | It closes (3) with ghost lanes. It closes trap persistence with poison bits, but not NaN. It replaces a `poly` whose notes lane changed, which cuts that instrument's release tails after 5 ms. Its `Hold` lets lane previews keep flowing during an export. |
| **Buildable in green slices** | Yes. Slice 1 touches only `core/runtime` and `core/play`, so it can start while the data-plane workflow owns `core/session`, `crates/server`, `crates/gates` and `web`. Names land after the arena. | Yes, but slice 1 starts in `core/session/src/text.rs` and `tx/delta.rs`, which the data-plane workflow is editing now. |

### 0.2 Decision

The base is **design-simple**. It has the fewest concepts, the slice order that fits the data-plane
workflow, exact fades, per-track tap gates, and keep-and-release for a rewired instrument.

**Grafted from design-robust:**

1. **The cycle rule** (§6.6). Without it the base can't prepare an effect reorder.
2. **Verify, then move** (§7.2). The audio thread checks the base plan and a 64-bit shape hash for
   every move, and refuses the whole activation if any check fails. The actor recovers through the
   existing `Way::Replace`.
3. **Lanes are engine state** (§8). Schedulers, control lanes and audio players persist by `LaneAddr`.
   Audio-lane edits and assets take `SwapLane`, with a 5 ms crossfade from the old renders. A lane that
   only a ghost still reads stays as a ghost lane for the window. This removes the base's audio box,
   its free-running ghost and `PlanInfo.audio`.
4. **Cells carry across an activation** (§9). A continued cell whose value changed is applied as a
   `SetCell` from the old value would be, so a smooth control ramps. That makes the server's value
   fast path redundant; deleting it is slice 2's last commit, behind a measurement.
5. **A plugin's last-sent parameter values continue with it** (§10.1), so a no-op activation sends
   nothing to a plugin.
6. **`Hold` for export** (§10.5), in place of the base's lend of the whole `Live`. A lend moves the
   engine across threads with a timeout race. During a long export it also blocks the queue, where the
   data-plane workflow's lane previews will travel. Recovery from a stall is a plain `Hold(false)`.
7. **A plugin restart is prepared at once**, from the instance's own saved state (§10.3).
8. **Leak counters and a churn test** (§12.7): `LIVE_KERNELS`, `Engine.leaked`, and the plugin
   `live()` count.
9. **G7f's extra metrics** (§11): reserved address space, and drop contention measured against
   today's whole-store drop.

**New in the merge:** one `tainted` flag per plan (§7.7). It generalises robust's trap poison to
non-finite output: a plan that trapped or put a NaN at the output keeps no kernel state at its next
activation.

**Dropped:**

- from robust: the `Plan`/`Data`/`Next`/`PlanIndex` split and an engine-owned transport (churn with no
  audible benefit); per-slot poison bits (one flag is enough); batched drops built unconditionally
  (only if G7f shows contention worse than today's drop on every edit); a poly shape that includes its
  lane; identity fixes before the arena (coordination);
- from simple: `Shape::Audio`, the ghost audio node and the lend.

---

## 1. The design on one page

**One rule.** Every stateful node of a plan holds its instance in a box. An activation moves boxes
from the plan that plays into the new plan, at a block line, by a move list computed off the audio
thread. Whatever isn't moved leaves with the old plan and is dropped off the audio thread. The boxes
are:

- a kernel instance, which owns its own wasmtime `Store`;
- a whole `poly` voice pool;
- a plugin instance;
- a feedback ring.

Lanes (notes schedulers, control lanes, audio players) are moved the same way, by `LaneAddr`. "The
arena" is just the boxes in the playing plan's slots. There is no `RuntimeId` type and no map.

**Identity** is step 6's `GraphDelta` node names, compared between what plays and what is accepted.
`m2_session::tx::continues` joins those names to flat node indices. The runtime then keeps a node only
when its shape matches: the kernel id and its package's wasm SHA-256, the channels, a poly's blanked
voice structure, a plugin's id and version. Slice 2 uses the names as built: untouched tracks are in
unchanged files, so their names already match. Slice 3 fixes four naming defects, so wrapping an
instrument in an effect keeps the instrument.

**Fades** go only on changed edges into kept nodes and on the song output. They are linear over 5 ms,
and bit-exact when the old and new signals are equal. A replaced or removed node runs on as a *ghost*
for that window, with its notes ended, then is retired. No instance is ever processed twice. An edit
that would make the window's graph cyclic (a reorder of kept effects) replaces the nodes on the
cycle.

**Engine messages:**

| Message | Carries | Status |
|---|---|---|
| `Activate` | a prepared `Renderer` with its `Entry` | as today |
| `SwapLane` | notes, control and now audio lane data | audio is new |
| `Transport` | a command | as today |
| `Panic` | — | as today |
| `Hold(bool)` | pause or resume, for export | new |

These become `Activate`s that keep every instance they can: a tempo change, a notes lane that outgrows
its scheduler, a plugin restart, and, after slice 2, a committed value. `LoadState` is a main-thread
`load_state` call plus a flag. There is no `SetArrangement` and no `LoadState` message.

**Export** of a song with plugins:

1. `Hold(true)` fades the engine out and stops it.
2. The export thread runs RENDER-1's host calls on the live plugin instances, shared by `Arc`, and
   renders with fresh kernels.
3. It restores each instance's own saved state, then `Hold(false)` fades the engine back in where it
   was.

**Store:** one per kernel instance. G7f measures the cost first, with a rule written in advance. The
fallbacks are the pooling allocator or a smaller memory reservation, never a shared store.

**Build order**, four slices, each green:

1. Movable instances, no audible change (can start now).
2. The arena.
3. Names that survive edits.
4. Plugins persist: `LoadState`, restart, export.

---

## 2. What a producer must not notice, and what keeps it so

| # | Heard | How it would happen | What rules it out | Checked by |
|---|---|---|---|---|
| P1 | A tail cut on a track nobody touched | a rebuild of every instance (today); names that shift | instances move and are never rebuilt (§7.2); an untouched node keeps its inputs, so no fade touches it (§6.3) | G7b, bit for bit per track |
| P2 | A click at the edit | a one-sample switch; a fresh node's first sample; a kept node whose source changes | every changed edge into a kept node, and the output, fades over 5 ms; a fresh node can only reach a kept node through a changed edge, since it continues nothing (§6.3) | G6 `step`; `an_inserted_effect_fades_dry_to_wet` |
| P3 | A smooth control jumping | a continued cell whose value changed, written as a step at activation | cell carry: applied as a `SetCell` from the old value (RENDER-22a) (§9.1) | `a_committed_smooth_value_ramps_through_an_activation` |
| P4 | A stuck note | a kept instrument rewired to another lane; a scheduler replaced under a kept consumer; note ids colliding in a plugin | schedulers persist by `LaneAddr`; a rewired consumer is released at entry; note ids are engine-wide (§8.1) | G4 `stuck`; `random_edits_and_jumps_never_strand_a_gate` with structural edits |
| P5 | A plugin losing its patch | a new instance where an old one should continue; a skipped load; code-owned parameters not re-sent; export leaving it offline | plugins move by name and shape, keyed ones also by state path; a state change loads into the kept instance; the host's skip check tracks what it *holds*; a `reloaded` flag re-sends code-owned parameters; export restores the instance's saved state and realtime mode (§10) | G7e; `load_then_restore_is_never_skipped` |
| P6 | An export different from what was committed | the live engine processing an instance during the export; no reset | `Hold` proves no plugin is called before any host call; reset on the render thread after activate; kernels fresh (§10.5) | G7e probe: borrowed export equals a fresh one bit for bit |
| P7 | Stale kernel memory after a rebuilt package | matching by kernel id only | the shape includes the package's wasm SHA-256, checked at prepare and again on the audio thread (§5.2, §7.2) | G7d, with an identical-bytes control |
| P8 | A song silent forever after a NaN | a NaN'd filter or a trapped kernel kept across edits | a tainted plan keeps no kernel state at its next activation (§7.7) | `a_plan_that_went_non_finite_keeps_no_kernel` |
| P9 | A dropout at the edit | today's 2× DSP for 2 blocks; allocation on the audio thread | one plan; only ghosts run twice; every list is sized at prepare (§7) | `no_alloc.rs` extended; G7b's activation-block time |
| P10 | Memory creeping up | a shared Store; retirements leaked | one Store per instance; the room to retire is checked before an activation applies (§7.6) | the churn test (§12.7) |
| P11 | Playback jumping back, or an edit never arriving | a copied transport going back; a dropped or coalesced activation | the transport is copied and retimed by beat; activations never coalesce; a refused one is recovered by `Way::Replace` (§7.5) | G5 `Sampler`; `an_activation_against_another_plan_is_refused` |

Two rules carry most of this:

- **The plan owns wiring; the boxes own state.** Wiring is built fresh off the audio thread for every
  plan, and only state moves. Nothing on the audio thread rewires anything.
- **Verify everything, then move everything.** An activation applies whole or not at all.

---

## 3. Identity

### 3.1 Names, not numbers

ID-13 asks the session to number each site, `SiteId(u32)`, at every accepted code revision. This
design builds no counter:

- **The engine only ever compares two revisions:** what plays (`Plays.revision`) and what is accepted.
- **Step 6 already names every node between any two revisions** (`delta_between`,
  `core/session/src/tx/delta.rs`). On the old side every point is `Base(own offset)`, so the old
  side's names depend on that revision alone.
- **A session counter would be history:** it must survive failed builds, undo and reopen. Put into
  delta names (WRITE-8's "From step 7, SiteIds replace the mapped ranges"), it would make `DeltaHash`
  differ between two processes. That breaks step 6's gate and `Seen(hash)`.

The contract's derived ids are spelled from delta names:

| Id | Spelling |
|---|---|
| **SiteId** | (file, call place) as a point in the comparison's coordinates. Through slice 2 the call place is the call's first and last byte, as built. From slice 3 it is the point of its `(`. |
| **InstancePath** | the module key and the frame chain. Each frame is `key:<k>` or (slice 3) `<ComponentId>@<file>:<point>#<occurrence>`. From slice 3, nothing above the innermost `key()` frame is part of it. |
| **RuntimeId** | the full name of a kernel, plugin, `poly` or feedback node: InstancePath / `<op>@<file>:<point>` / `#<n>`. It never leaves the session. |
| **ControlId** | the name of a declared control's const, `@<slot>` (built). |
| **CellId** | an index in the active plan's cell table (unchanged). |
| **A voice's RuntimeId** | its slot in its kept pool (RENDER-16). |

### 3.2 `continues` (slice 2, on the names as built)

The one new session function, in `core/session/src/tx/continues.rs`:

```rust
/// For each top-level node of the new revision's Flat: the node of the old revision's Flat it
/// continues, by name (ID-13, ID-15) and, from slice 3, with equal occurrence counts (ID-14);
/// keyed plugins left unmatched are then paired by state path (ID-8). Shapes are the runtime's job.
pub struct Continues { pub top: Vec<Option<u32>> }
pub fn continues(old: &Revision, new: &Revision) -> (GraphDelta, Continues);
pub fn identity(flat: &Flat) -> Continues;   // same code: node i continues node i
```

How it works:

1. Build the same `Coords::revisions(Edits::between(..))` per changed code file as `delta_between`,
   and call `delta::compare`, which already returns both sides' `Graph`s.
2. Expose the `ids: HashMap<String, Name>` that `Side::scope` builds per module (about 10 lines), so
   each `Graph` maps (module key, IR node id) to its name.
3. Join, for each top-level `FNode` of the new `Flat`: split `FNode.name` (`<module>/<node id>`), look
   up its name, look for the same name on the old side, then find the old `FNode` by its
   `<module>/<id>`. Pure nodes (consts, lanes, time, `Missing`) are joined too: the runtime uses them
   to decide whether an input's source is "the same source" (§6.3).
4. Nodes inside voices aren't joined: a `poly` moves whole (§5.1).

Where it is called:

- **The actor's `reach`** calls `delta_between` on every settle today. It calls `continues` instead and
  keeps the delta, so the extra cost is the join, about 1 ms at p50 (*estimate*).
- **When the code is the same** (a tempo change, a lane that outgrew its room, a restart, a recovery),
  the actor uses `identity` and names nothing.

Why slice 2's names are enough for the gate:

- Untouched tracks are in unchanged files, so their names are `Base` points on both sides. Package
  (std) sites are always `Base`.
- So every node of an untouched track, and the mixer in an untouched `song.ts`, continues.
- The edited file's calls whose first or last byte falls in a line hunk are renamed. For an effect
  wrapped around an instrument, that includes the instrument (identity notes F2), so in slice 2 that
  one track restarts its instrument, as it does today. Every other track keeps everything.

### 3.3 Names that survive edits (slice 3)

The four defects from `understand-identity.md` §3, plus two passes. All are on the engine's path
(`delta_between`), so every writer gets them, agents included (PLANE-1).

| Fix | What | Where | Size |
|---|---|---|---|
| **F1** | refine each line hunk with a token-level Myers diff: identifiers and numbers, strings, single punctuation characters, with whitespace and comments in the gaps; reuse the generic `kept_lines`; past `DIFF_LINES` tokens keep the hunk whole | `core/session/src/text.rs` `Edits::between` | about 80 lines |
| **F2** | name a call by the point of its `(`: a new `Site.open: u32`, found by scanning from the callee span's end (or the type arguments') past whitespace, comments and `?.`. `Facts.spans` keeps the whole range, so judging `Confined` is unchanged | `core/script/src/sites.rs` `visit_call_expression`; `delta.rs` `Side::place`, `Side::spell` | about 35 lines |
| **F3** | put the ComponentId in each chain segment, so a component swapped at one `(` is new and an alias (`delay as echo`) is not (PKG-9) | `delta.rs` `Side::chain` | about 5 lines |
| **F6** | `key()` pins: drop every segment above the innermost `key:` segment, since keys are unique per parent key frame (`keyFrames` in `core/script/sdk/core.ts`) | `delta.rs` `Side::chain` | about 10 lines |
| **F5** (ID-14) | `Facts.counts: Vec<u32>`: for each chain segment with an occurrence, how many frames share its (parent, site), then the size of the node's own `#n` group; `continues` keeps a pair only when the counts are equal | `delta.rs` `chain`, `name_all`; `continues.rs` | about 25 lines |
| **F7** | keyed plugins, second pass: an unmatched new `FOp::Plugin` with `state: Some(path)` takes the unmatched old one in the same module with the same path, id and version; unambiguous by `plugin.state-collision` | `continues.rs` | about 20 lines |

- **The delta's hash changes its spelling once.** It stays deterministic, so step 6's two-process gate
  holds once its expected names are updated.
- **Optional, off the critical path:** `crates/editor/src/controls.rs`'s `carry_text`, `carry_ends`
  and `carry_lines` become `text::Edits::between(old, new).carry(range)`, one correspondence for
  leases, cables and undo (WRITE-12, WRITE-22). `m2_ir::control_id`, which only a test uses, is
  deleted.

### 3.4 Properties of the match

- **Injective.** Names are unique (the `debug_assert` in `scope`, and
  `delta_names_are_unique_on_every_fixture`). The second pass takes only unmatched nodes on both
  sides. `prepare` checks again, and a `from` that repeats is a bug assertion.
- **Never across modules.** Every name starts with the module key. A step 8b key rename can add a
  third pass over `ModuleChange::Rekeyed`.
- **Shape-gated.** A wrong name match can only pair same-kind, same-shape instances (§5.2).
- **Ambiguity resets.** A changed count resets the whole group (slice 3). A name with no partner is
  fresh.
- **Which way errors lean.** A false *keep* hands a same-shape neighbour's state over for a few
  seconds, and the consumers' changed inputs still fade, so it can't click. A false *reset* cuts a
  tail, which is what G7b looks for. After slice 3, the remaining false resets are a reformat of a whole
  track call (only that track) and a cut-and-paste (deferred, with ID-13's rule 4).

### 3.5 A rewired note consumer (identity note F8)

A kept `poly` or plugin whose notes input now reads another lane (`lane.notes("a")` → `"b"`) holds
notes the new lane's scheduler doesn't know. They would stick.

- `prepare` compares the old and new `LaneAddr` of each kept note consumer.
- When they differ, the node goes on `Entry.release`, and its notes end at entry (§7.2 step 6).
- A kept `poly` stays kept, so its voices go into their release and ring out whole.
- No delta field is needed: it is a comparison of two `FOp`s.

---

## 4. `Plan` split from `DataSet`

### 4.1 Today

`Renderer` (`core/runtime/src/lib.rs:1237`) is everything: one `Store`, the `Graph` (exec, buffers,
versions), `Cells`, `lanes: Vec<LaneSlot>` and their `slots`, the tempo map, the arrangement, the
transport and the clock. `DataSet` already exists (`core/song/src/data.rs:64`). Any structural edit
rebuilds all of it (`Renderer::build_live`).

### 4.2 After step 7

```rust
// core/runtime/src/lib.rs
/// What an engine plays: one plan, the lanes its nodes read, and the song position.
pub struct Renderer {
    plan: Plan,                    // structure from the Flat, with the boxes it holds
    lanes: Lanes,                  // from the DataSet: schedulers, control lanes, audio players
    tempo: TempoMap,
    arrangement: Arc<Arrangement>,
    sample_rate: f64,
    transport: Transport,
    clock: u64,
    next_note: u32,                // engine-wide note ids (§8.1)
    tainted: Arc<AtomicBool>,      // a trap or a non-finite output since it was prepared (§7.7)
    frames, runs, pending, delivered, strict, non_finite, broken, trace, dropped,   // as today
}

// core/runtime/src/plan.rs (new)
pub(crate) struct Plan {
    id: PlanId,                    // today's `generation`, from the same process-wide counter
    graph: Graph,                  // exec wrappers, bufs, versions; `graph.order` is the window's order while one runs
    order_after: Vec<usize>,       // the order once the window ends: ghosts left out
    cells: Cells,                  // per plan, as today (RENDER-23)
    out: PortRef,                  // may point at a fade node
    entry: Option<Entry>,          // what take_over does (§6.9); None for a fresh plan
    ghosts: Range<u32>,            // exec indices of the ghosts
    fade: u32,                     // round(0.005 × rate) frames; 0 when nothing fades
    faded_from: Option<u64>,       // the engine clock at entry, set by take_over
    leftovers: Leftovers,          // room for every ghost exec and ghost lane, for the window's end
    info: Arc<PlanInfo>,           // never read on the audio thread
}

pub(crate) struct Lanes {
    slots: Vec<LaneSlot>,          // live lanes, then ghost lanes (§8.2)
    refs: Vec<LaneRef>,            // module, lane, kind: what `lane_slots()` gives
    live: usize,                   // slots[live..] are read only by ghosts
}

pub(crate) enum LaneSlot {
    Notes(Option<NoteScheduler>),
    Control(Arc<ControlLane>),
    Audio(Option<Box<AudioPlayer>>),   // today's AudioNode state, plus its renders (§8.2)
}
```

What is built from what:

- **`Plan`** from the `Flat` and the packages only: `Builder::graph`, the exec wrappers, fresh boxes,
  `cells(flat)`, and the ghost and fade nodes.
- **`Lanes`** from the `DataSet` and the tempo map: `NotesData::build`, `data.control()`, and
  `AudioLane::build` through `audio::Cache`.
- **A change of data alone** never builds a `Plan`: `SwapLane` replaces lane data in the running
  `Lanes`.
- **The contract's `DataSet { arrangement, lanes }`** is `m2_song::DataSet`, unchanged.

**Why keep the name `Renderer`.** `Engine`, `Player`, `m2_editor::render::Render`, the CLI and about
40 tests use it. Under it, the contract's split is visible as `plan` and `lanes`. A
`Plan`/`Data`/`Engine` re-split with the transport in the engine (design-robust) would move the same
fields and change every caller, for no audible difference. The transport is copied and retimed at
entry, as `enter_from` does today.

### 4.3 Constructors

```rust
impl Renderer {
    /// Offline, CLI, tests: every instance new. Unchanged signature.
    pub fn new(flat: &Flat, data: &DataSet, packages: &PackageSet, rate: u32) -> Result<Renderer>;
    /// Everything else. Replaces `build_with` and `build_live`.
    pub fn prepare(flat: &Flat, data: &DataSet, packages: &PackageSet, rate: u32,
                   cache: &mut audio::Cache, owner: &Owner, basis: Basis) -> Result<Renderer>;
    pub fn info(&self) -> Arc<PlanInfo>;
}

pub enum Basis<'a> {
    /// Every instance new: an offline render, `Player::open`, a rate change, a new player.
    Fresh,
    /// Move whatever continues from the plan the engine will be playing when this one arrives.
    Keep {
        old: &'a Flat,                    // Plays.revision's Flat
        info: &'a PlanInfo,               // the last plan sent
        cont: &'a Continues,
        changed: &'a LaneChanges,         // lanes the delta lists, and whether the tempo changed
        restarts: &'a HashMap<Handle, Option<Arc<Blob>>>,  // §10.3: restart, from this saved state
        wipe: bool,                       // §7.7: the old plan is tainted
    },
    /// An offline render sharing live plugin instances (RENDER-1, §10.5); kernels fresh.
    Borrow { plugins: &'a HashMap<usize, Arc<PluginInstance>> },
}
```

`Renderer::starting_at` (range render) and `seek` (offline replay) are unchanged.

---

## 5. The arena: boxes, shapes, `PlanInfo`

### 5.1 What holds state

The arena holds these four boxes, and only these (`Exec` in `core/runtime/src/lib.rs`):

| `Exec` variant | Box (moves at entry) | Plan side (new each plan) | Shape |
|---|---|---|---|
| `Kernel(KernelNode)` | `inst: Option<Box<KernelInst>>`: its own `Store<StoreLimits>`, `Memory`, typed funcs (`reset`, `process`, `active`), `in_ptrs`, `out_ptrs`, `out_channels`, `name`, `shape: u64`, and the debug ABI-1 copy | `inputs: Vec<Input>`; `seen: Vec<u64>`, starting at `COPY`, so each port input is copied into memory once at its first call | `Kernel { id, wasm: Sha, channels }` |
| `Poly(PolyNode)` | `pool: Option<Box<VoicePool>>`: `voices: Vec<VoiceSlot>` (each a voice `Graph` with its kernel instances, `note`, `born`, `dirty`, `last_peak`), the pool's `born`, `shape: u64` | `lane`, `out`, `captures: Vec<PortRef>`, `plan: Vec<Planned>` (sized by the lane's caps) | `Poly { voices, channels, voice: Arc<Voice> (blanked), wasm: Vec<Sha> }` |
| `Plugin(PluginNode)` | `inst: Option<Arc<PluginInstance>>` (with `shape: u64`) | `audio_in`, `notes`, `params: Vec<(u32, Input, Option<f32>)>`, `keys`, `ended`, `release`, event lists (sized by caps), `status`, `ghost: bool` | `Plugin { id, version, channels, state: Option<Sha> }`; the state hash goes in slice 4 |
| `Feedback { input, ring }` | `ring: Box<[f32]>`, swapped | `input: PortRef` | `Feedback { channels }` |

- **Nothing else holds state that outlives a plan.** That covers `Const`, `Time`, `Control`, `Notes`,
  `Missing`, `NoteField`, `Capture`, the new `Fade`, and `Exec::Audio { lane }`, which now copies its
  lane player's block (§8.2).
- **A kernel's `Input::Const` values are already in its memory.** An unchanged one is the same value.
  A changed one is a changed edge, so the kernel reads a fade node instead (§6.3).
- **A `poly` moves whole.** Its voice graphs, their kernels, notes and buffers are one box.
  - The p50 voice pools are about 97% of all instances, so moving each as one pointer keeps prepare and
    entry O(top-level nodes).
  - An edit inside an instrument's own voice graph replaces that instrument, which is today's
    behaviour. Voice-by-voice matching can come with mono and legato.
- **Voice captures** are `Exec::Capture(u16)`, an index into `PolyNode.captures`, which is plan-side.
  So a moved pool needs no patching, and a changed capture source is just a fade node that
  `captures[k]` points at.
- **A plugin node's room** (`keys`, `ended`, `release`, event lists) depends on the caps of the lane it
  reads, so it stays plan-side. It is copied in at entry within its new room, which is today's
  `carry_from` copy without the adoption. The last-sent values in `params` are copied by parameter id
  (§10.1).

### 5.2 Shape

```rust
#[derive(Clone, Debug, PartialEq, Hash)]
pub enum Shape {
    Kernel   { id: String, wasm: Sha, channels: u8 },
    Plugin   { id: String, version: String, channels: u8, state: Option<Sha> },   // `state` until slice 4
    Poly     { voices: u32, channels: u8, voice: Arc<Voice>, wasm: Vec<Sha> },
    Feedback { channels: u8 },
}
impl Shape { pub fn hash64(&self) -> u64; }   // stored in each box when it is created
```

- **`wasm`** is the SHA-256 of the kernel's package wasm module, from `Package.files`. The new
  `PackageSet::module_sha(kernel_id)` is about 8 lines.
  - It is a content hash, so two `PackageSet`s loaded from the same bytes give equal shapes although
    their wasmtime `Module`s differ (G7d's control).
  - std is one module for every kernel it has, so rebuilding `std.wasm` changes the shape of every std
    instance. That is safe.
  - The hash covers the module's ABI version (`m2_abi()`). In ABI v0 the only init-time setting is
    `channels` (`k_init`). So this is RENDER-25's "kernel id, module hash, channel counts, state ABI
    and settings".
- **A blanked `Voice`** is the voice `Flat` with every `FOp::Const` value, every origin, `src` and
  name, and every capture target cleared; capture channel counts are kept.
  - It is the server's `bare()` from `value_writes`, applied to one voice.
  - `Input::Const` values stay in it, so a plain number edited inside an instrument body replaces the
    instrument (owner question Q9). Declared controls are cells and don't.
  - The `Arc<Voice>` is the one in the `Flat`, so the blanked copy is made once per poly at prepare.
- **A plugin that asked for a restart** doesn't match (§10.3).
- **The hash only guards the audio thread.** `prepare` compares `Shape`s fully; the audio thread
  compares `hash64`s (§7.2).

### 5.3 `PlanInfo`: what the next prepare reads

```rust
pub struct PlanInfo {
    pub id: PlanId,
    pub rate: u32,
    pub shapes: Vec<Option<Shape>>,            // by top-level Flat node; None: holds nothing
    pub lanes: Vec<LaneInfo>,                  // by lane slot: { addr: LaneAddr, kind, caps: Option<Caps> }
    pub plugins: Vec<PluginAt>,                // { node, inst: Arc<PluginInstance>, state: Option<Vec<String>> }
    pub index: HashMap<String, usize>,         // FNode.name -> node: gates' taps, export's borrow
    pub cell_slots: Arc<CellSlots>,
    pub cell_map: Arc<CellMap>,
    pub tainted: Arc<AtomicBool>,              // the renderer's flag (§7.7)
}
```

- **Where it comes from:** `prepare` writes it, and the server keeps it as `Plays.info`. It replaces
  `Plays.{slots, caps, cell_slots, cells}` and `Open.carry`.
- **It is immutable, and describes the plan as it will be once its activation applies.** The next
  prepare reads only `PlanInfo` and the old `Flat` (`Plays.revision`), never the live plan, which
  belongs to the audio thread.
- **Why `Plays.revision`'s `Flat` is the right old `Flat`:** `adopt` moves it forward only when the
  graphs are equal but for constant values (`value_writes`), so its node indices are the playing plan's.
  After the fast path is deleted (§9.3), every code change activates, and the point is moot.
- **Plugin lifetimes:** the `Arc<PluginInstance>` clones keep an instance alive until the `PlanInfo`
  that names it is replaced. That happens at the next settle on the actor thread, where its destruction
  is queued to the plugin main thread, as today.

---

## 6. Prepare, off the audio thread

Inputs: the old `Flat` O, its `PlanInfo` I, the new `Flat` N, `cont`, `changed`, `restarts`, `wipe`.
`prepare` never touches a live box. A prepare that fails (for example, a plugin that won't load)
changes nothing that plays: its fresh instances drop on the actor thread, and the old plan plays on.

### 6.1 Lanes

For each lane that `lane_slots(N)` lists, by `LaneAddr` against `I.lanes`:

- **In I with the same kind:** a `LaneMove { from, to, swap, grow }`.
  - `swap` is new data when the delta lists the lane or the tempo changed: an `Arc<NotesData>`, an
    `Arc<ControlLane>`, or an `Arc<AudioLane>` rendered through the cache. Otherwise it is `None`.
  - `grow` holds a fresh `NoteScheduler` sized for the new data, when its `needs` exceed the old caps.
- **Not in I:** a fresh entry built here: a scheduler, a control `Arc`, or an audio player with its
  renders.
- **Ghost lanes:** each control or audio lane that a ghost reads (§6.4) and N doesn't is appended after
  `live`, as a `LaneMove` of the old entry. Ghost consumers read no events, so notes lanes never need
  this.

### 6.2 Matching stateful nodes

Each node `i` of N that holds a box is **kept** when all of these hold:

- `cont.top[i] == Some(j)`;
- `I.shapes[j] == Some(shape(N, i))`;
- `j` isn't claimed already;
- `j`'s plugin handle isn't in `restarts`;
- `wipe` is false, or the node is a plugin.

A kept node claims `j`, and `Move { from: j, to: i, shape: hash64 }` goes on the move list.

Every other stateful node gets a fresh box:

- **Kernels:** `Store::new`, `Instance::new`, typed funcs, `k_init`, the port checks and the prewarm
  reset, as today, with one store each.
- **Plugins:** `host.create` (from the restart blob when there is one), then `activate`, on the plugin
  main thread while the old plan plays.
- **Polys:** every voice graph built.
- **Feedback:** a zeroed ring.

**Pure nodes** (`Const`, `Time`, `Lane`, `Missing`) hold no box. Each one *continues* when
`cont.top[i] == Some(j)` and the op kind is the same. Two helpers:

- `same(i)` is `j` for a kept or continuing node, else `None`;
- `new_of[j] = i` is its inverse.

### 6.3 Edges: which inputs fade

For each kept node `i` and each continuing `Missing`, compare each input `p` with old node `j`'s input
`p`. The inputs are positionally equal because the shapes are equal. The inputs compared are:

- a kernel's inputs;
- a plugin's `in` (parameters aren't faded: a changed parameter source is a parameter step, which
  plugins smooth themselves);
- a feedback's `input`;
- a poly's captures.

An input is **unchanged** when one of these holds:

- both are ports, `same(new src) == Some(old src)`, and the port and its channel count are equal;
- both are `Const` with equal bits;
- both are `Unset`.

Anything else is a **changed edge** `(i, p, old, new)`. The song output is checked the same way: `N.out`
against `O.out`.

What follows from this:

- **The boundary.** A fresh node continues nothing, so it is never on an unchanged edge into a kept
  node. Every fresh subgraph that reaches a kept node or the output does so through at least one fade.
  Fresh instances may start from any init state without a click.
- **Untouched nodes.** A kept node whose inputs are all unchanged reads exactly what it read before. So
  an untouched track is bit-identical through the activation (G7b).
- **A continued cell is an unchanged source,** whatever its value. A changed value is applied by cell
  carry (§9.1), never by a fade.

### 6.4 Ghosts

1. The worklist starts with the old source of every changed edge whose `new_of` is `None`.
2. Each old node `j` taken from it gets a ghost index and pushes each of its port inputs (feedback
   inputs and captures included) whose `new_of` is `None`. The closure stops at old nodes that
   continue.
3. Each ghost is appended to the new plan's exec as a clone of `O[j]`'s wiring. Each input is remapped
   to `new_of[src]`, or to that source's ghost index. A const or cell input becomes
   `Input::Const(old value)`.
4. **Stateful ghosts** get `Move { from: j, to: ghost }`. A ghost `poly` or ghost plugin is flagged
   `ghost`: its notes end at entry, and it reads no events after them.
5. **Pure ghosts:**
   - a `Const` keeps its old value;
   - `Time` is `Time`;
   - a control `Lane` reads its slot, live or ghost (§6.1).
6. **Not every replaced node runs.** A replaced node that no fade needs doesn't run at all; it leaves
   with the old plan.
7. **When `wipe` is set** there are no ghosts: the old side of every changed edge is silence (§7.7).

### 6.5 Fade nodes

For each changed edge, append `Exec::Fade { old: Input, new: Input, channels: u8 }`:

- `old` is the remapped old input, or `Const(0.0)` when it was `Unset` or when `wipe` is set;
- `new` is the new input;
- `channels` is the new source's count, and a mono side is duplicated, as kernel input copying does.

The consumer's input `p`, or `out`, is pointed at the fade node. It stays there after the window as an
exact copy until the next plan drops it, so the audio thread never rewires anything.

### 6.6 Order, and the cycle rule

- **No ghosts and no fades:** the order is `N.order`, unchanged. A no-op activation runs exactly the
  offline order.
- **Otherwise:** run Kahn's algorithm, lowest index first, over N plus ghosts plus fades. Feedback
  inputs aren't counted, which is `check.rs`'s rule. `order_after` is that order without the ghosts.
- **Order changes no bits.** Within a block, a pure node's output depends only on its inputs and its
  state, and feedback rings are written after every node has run (`Graph::process`, `lib.rs:960`). So
  any topological order gives the same bits.
- **The cycle rule.** An edit that reverses an edge between kept nodes makes the window's graph cyclic.
  - Example: old `x→A→B→y`, new `x→B→A→y`, both kept. `A` reads `Fade(x, B)`, while `B` reads
    `Fade(A, x)`.
  - When Kahn's algorithm leaves nodes over, every kept node in a leftover strongly connected component
    is demoted to fresh, and §6.3 to §6.6 run again.
  - This is bounded by the number of kept nodes.
  - The reordered effects lose their tails, and nothing else does (owner question Q4).

### 6.7 Cells

- `cells(N)`, `CellMap::of` and `CellSlots::new`, as today.
- `Entry.cells: Vec<(CellId, CellId)>` lists (new, old) for each top-level const that continues.
- `Entry.voice_cells: Vec<CellId>` lists every voice cell (`CellAt::Voice`) of a kept poly. Those
  constants live in the moved voice graphs.
- Both are compared at entry (§9.1).
- The server then sets each live lease's value with `Renderer::set_cells`, as today.

### 6.8 Plugins in prepare

- **New instances** are created, state-loaded and activated on the plugin main thread while the old
  plan plays (RENDER-25). The actor publishes `loading: [names]` meanwhile (slice 4).
- **A kept instance's state change** is `load_state` before the `Activate` is sent (§10.2, slice 4).

### 6.9 The entry, all sized here

```rust
pub(crate) struct Entry {
    after: PlanId,                     // the plan it was prepared against
    moves: Vec<Move>,                  // Move { from: u32, to: u32, shape: u64 }: kept nodes, then ghosts
    lanes: Vec<LaneMove>,              // { from, to, swap: Option<LaneSwap>, grow: Option<NoteScheduler> }
    cells: Vec<(CellId, CellId)>,      // §6.7
    voice_cells: Vec<CellId>,          // §6.7
    release: Vec<u32>,                 // new exec indices whose notes end at entry: ghosts, rewired (§3.5)
    pub stats: Stats,                  // kept, fresh, ghost and fade counts by kind (gates)
}
```

`Leftovers` (on `Plan`) has room for every ghost exec and every ghost lane. It is where they go at the
window's end.

### 6.10 Worked examples (p50, track `t02`, pluck into the mixer's gain)

| Edit | Kept | Fresh | Ghosts | Fades |
|---|---|---|---|---|
| wrap pluck in `delay(…)` (slice 3 names) | pluck's `poly`, gain, everything else | delay | none | `gain.in`: pluck → delay |
| the same, slice 2 names | everything but t02's pluck and delay | pluck, delay | the old pluck (released) | `gain.in`: old pluck → delay |
| remove that delay | pluck, gain | none | delay, reading pluck's output | `gain.in`: delay → pluck |
| `lowpass(` → `highpass(` at one call | everything but the filter | highpass | lowpass | the filter's consumer's input |
| reorder `comp(eq(x))` → `eq(comp(x))` | everything but comp and eq | comp and eq (demoted by the cycle rule) | the old comp and eq | the consumer's input |
| a tempo change | everything | none | none | none (lanes swap, §8.4) |
| a rebuilt `std.wasm` (G7d) | plugins and lanes only | every kernel and voice pool | the whole old kernel graph | the song's output |

### 6.11 What an activation costs at p50 (*estimates*)

| Part | Today | After |
|---|---|---|
| Kernel instances built | about 6,100 | the new ones: 1 to 3 for an added effect, about 156 for a replaced pluck |
| Plan-side wrappers and buffers | inside the build | about 2,500 top-level nodes, about 2.5 MB zeroed: 1 to 3 ms |
| `continues` | the delta alone | plus about 1 ms for the join |
| Audio-thread entry | `enter_from`, `carry_from` | about 300 moves, about 100 lane moves, about 1,500 cell compares: tens of µs |
| CPU during the 5 ms | twice the whole song | the song plus the ghosts |
| Teardown on `m2-play-drop` | about 6,100 instances unmapped | the replaced ones, plus plan buffers |

---

## 7. Activation, on the audio thread

### 7.1 Messages and readiness

```rust
pub enum Msg {
    Activate { renderer: Box<Renderer> },                         // carries its Entry
    SwapLane { generation: u64, slot: u32, data: LaneSwap },      // LaneSwap gains Audio(Arc<AudioLane>)
    Transport(Cmd),
    Panic,
    Hold(bool),                                                   // §10.5
}
```

**Order:**

- `Transport` applies as soon as it is read.
- `Panic` also applies at once, except while held or holding: then it is deferred to the resume, so no
  plugin is touched.
- `Activate`, `SwapLane`, `Hold(true)` and `Hold(false)` apply in strict order, as structural messages
  do today. A `Hold(false)` sent while its `Hold(true)` still waits can't overtake it, so the engine
  can't be left held. Nothing queued during an export waits for "not held": the actor sends no
  `Activate` then, and `SwapLane` applies while held.

**An `Activate` is ready** when all of these hold:

- no fade window is running;
- the engine isn't held or holding;
- no audio player it swaps has a crossfade from an earlier swap still running (at most 5 ms);
- the stash has room for 3 retirements: the old renderer now, the leftovers at the window's end, and
  one to spare.

Otherwise it waits at the head of the queue, as today, for at most 2 blocks at 48 kHz.

**`Hold(true)`** is ready when no window is running, and **`Hold(false)`** is always ready. **An audio
`SwapLane`** is ready when its player has no crossfade from an earlier swap running.

### 7.2 `Renderer::take_over(&mut self, old: &mut Renderer) -> bool`

This runs in order, with no allocation, no lock and no drop. It verifies everything first.

**Verify, mutating nothing.** The activation is refused if any of these fails:

1. `entry.after == old.plan.id`.
2. For every `Move { from, to, shape }`:
   - `from < old.exec.len()`;
   - the old box is filled;
   - the old and new exec variants agree;
   - the old box's stored `shape` equals `shape`;
   - the new slot is empty.
3. For every `LaneMove`:
   - the old slot is filled and of the same kind;
   - a notes `swap` fits the moved scheduler's caps.

**Then move:**

1. **Boxes.** For each `Move`, `self.exec[to].take_from(&mut old.exec[from])`:
   - *Kernel:* `inst = old.inst.take()`.
   - *Poly:* `pool = old.pool.take()`.
   - *Plugin:*
     - `inst` is moved;
     - `keys`, `ended` and `status` are copied within their room;
     - the last-sent values in `params` are copied by parameter id, a merge walk over two lists sorted
       by id;
     - for a node flagged `ghost`, or on `release`, the keys go to `release` instead, to get a NoteOff
       at its first call.
   - *Feedback:* `mem::swap` of the rings. The prepared ring goes back with the old plan.
2. **Lanes.** For each `LaneMove`:
   - **Plain:** `self.lanes.slots[to] = old.lanes.slots[from].take()`.
   - **With `swap`:** `swap(data)` into the moved entry. The replaced data is put back into the
     `LaneMove`, so it leaves with the spent `Entry`, never dropped here. An audio swap starts its
     crossfade (§8.2).
   - **With `grow`:** the prepared scheduler `take_state(&mut moved)`. That copies `sounding` (it fits,
     because it is bigger) and sets `swapped`. The small one goes back into `old.lanes.slots[from]`.
3. **Song position.**
   - `self.transport = old.transport`, then `retime(old.tempo → self.tempo)` and `enter()`, as
     `enter_from` does today.
   - `self.clock = old.clock` and `self.next_note = old.next_note`.
   - `Jump::Enter` re-seeks each scheduler's cursor without ending notes, which changes nothing for a
     moved scheduler.
4. **Cells.** Cell carry (§9.1).
5. **Release.** For each node on `release`:
   - a poly: every voice with a note gets `note.off = min(off, clock)`;
   - a plugin: done in step 1.
6. **Window.** Set `faded_from = Some(clock)` when the plan has fades.
7. **Retire the entry.** `mem::swap(&mut self.plan.entry, &mut old.plan.entry)`. The spent entry,
   holding the replaced lane data, leaves inside `old`.

`Engine::try_apply(Activate)`:

- **Refused:** the new renderer is retired whole, unplayed. `refused_activations` is counted, the
  `Applied` stamp gets `refused: true`, and a `debug_assert` fires. The old plan plays on.
- **Applied:** `old` is retired whole (`Retired::Renderer`). It takes every box nothing moved, its plan
  buffers and the spent `Entry`. Then `current = renderer`.

**`AudioPlayer` and `Jump::Enter`.** The player's `Jump::Enter` arm stops clearing `fade_in`. A fresh
player has none, and a moved one goes on with its own, so a fade-in in progress isn't cut and a no-op
activation stays bit-exact.

### 7.3 The window and its end

While the window runs, `Graph::process` follows the window's order: fade nodes mix, ghosts process,
and ghost lanes play.

At each block line, before messages, once `clock ≥ faded_from + fade` and the stash has room,
`current.end_fade()` runs:

1. each ghost's `Exec` is swapped with `Exec::Const(0.0)` into `leftovers`;
2. each ghost lane is swapped out the same way;
3. `graph.order = order_after`;
4. `Retired::Nodes(leftovers)` leaves the thread.

Without stash room it waits a block, and the ghosts play on, faded to nothing.

**Panic during the window** resets everything in the plan, ghosts included, and the window ends on
time. The engine never idles during a window.

### 7.4 Fades, exactly

`Exec::Fade { old, new, channels }`. For each channel and frame, with `k` the engine frames since
entry and `len = fade`:

```
y = if k + 1 >= len                     { n }      // after the window: an exact copy of the new source
    else if o.to_bits() == n.to_bits()  { o }      // identical signals stay bit-identical, -0.0 included
    else                                { o + (n - o) * ((k + 1) as f32 / len as f32) }
```

- **Linear,** like today's whole-output fade, because the two sides are mostly the same music (dry
  against wet, the same instrument).
- **The equality branch** makes a silent structural edit bit-exact through the window. That is what
  lets G7b demand bit identity.
- **After the window**, the fade node copies its new source only when that source's version changes,
  and takes its version. So a steady cell behind it still costs nothing per block (RENDER-22).

### 7.5 Message order (RENDER-28)

- **Activations are never coalesced.** Each move list is relative to the plan it was prepared against.
  The coalescing branch of `Engine::block` (`engine.rs:448`) is deleted.
- **The actor stays serial.** It prepares each activation against the plan it sent last. FIFO order
  makes that the plan playing when the engine reads it, so no acknowledgement is waited for.
- **`SwapLane` keeps its address `(generation, slot)`.** The actor resolves the slot by `LaneAddr` in
  `PlanInfo.lanes` of the plan it sent last. A swap for another plan is refused and retired, as today;
  strict order makes that a bug, and a `debug_assert`. Keeping the address unchanged keeps the
  data-plane workflow's lane previews (WRITE-22a) working as they are written.
- **Recovery from a refused activation.** The actor sees `refused` in the `Applied` stamps, and its
  next settle uses the existing `Way::Replace`: a fresh engine at the same beat, crossfaded by the
  player's `LiveFade`.
- **Recovery from a stall.** `Player::send` still reports `Stalled` after 500 ms and drops the message.
  The next settle uses `Way::Load` (a new player), as today. RENDER-28's text is updated to say so.

### 7.6 Retiring, and memory that can't grow

- **Every replaced object is retired off the thread.** It leaves in the old renderer, in the leftovers,
  or in a `LaneSwap`. The room for each is checked before the activation applies.
- **`mem::forget` stays as the unreachable fallback,** and is now counted as `Engine.leaked`. The churn
  test requires 0.
- **One `Store` per instance** (§11) means that dropping a `KernelInst` frees its memory. No retired
  instance is pinned by a live one.
- **Counters.** `KernelInst::new` increments a static `LIVE_KERNELS`, and its `Drop` decrements it.
  With `PluginHost::live()`, that is what the churn test compares.
- **Drops in batches.** The dropper (`drop_retired`, `core/play/src/lib.rs`) frees in batches only if
  G7f measures contention worse than today's drop of a whole store on every edit (§11.3).

### 7.7 Panic, idle, trap and non-finite output

- **Panic** resets the plan in place: kernels, plugins (`reset`), rings, voices, schedulers and audio
  players. The next block is `Enter`.
- **Idling.** `can_idle` and `woken` read the plan that plays. The engine never idles during a window
  or a hold.
- **Taint.** A trap (`broken`) or a non-finite sample at the output (`non_finite += 1`) sets the
  renderer's `tainted: Arc<AtomicBool>`, a single relaxed store with no allocation. `PlanInfo` holds a
  clone.
- **What the actor does with it.** When `PlanInfo.tainted` is set, the next prepare uses
  `Basis::Keep { wipe: true, .. }`:
  - every kernel, voice pool and ring is fresh;
  - there are no ghosts, since a broken plan's kernels must not be called again;
  - every changed edge fades in from silence;
  - plugins and lanes still move.
- **Why this matters.** One NaN anywhere makes the whole mix NaN, which is silenced at the output. Today
  any later rebuild heals it; with the arena a NaN'd delay ring would otherwise survive every edit
  except its own replacement. This rule restores today's behaviour for exactly that case: the next
  activation heals it, as today's next rebuild does, and `Panic` heals it in place, as today. No new
  trigger is added.

---

## 8. Lanes and arrangement switches

### 8.1 Schedulers persist, and note ids are engine-wide

- **One scheduler per lane moves from plan to plan,** so a kept consumer keeps receiving its lane's
  Offs. This deletes these, from `core/runtime/src/sched.rs`:
  - `NoteScheduler::adopt`, `raise_next_id` and `next_id()`;
  - the fields `next_id`, `released` and `adopted`;
  - `run`'s `release_all` argument, and `render_block`'s.
- **Note ids are engine-wide:** `Renderer.next_note: u32`, skipping 0. It is passed as `&mut` to every
  `scheduler.run` in plan lane order, copied at entry, and starts at 1 offline.
  - **It deletes** `PluginInstance.next_id` and `held_cap`.
  - **It is enough:** an id is never repeated into any plugin, whatever lane it reads.
  - **It changes no pure bit:** ids reach no kernel. A `poly` matches Offs by id and allocates voices by
    `born`, so the pure fixtures' offline renders stay byte-identical. Slice 1 checks that.
- **A replaced consumer** (a ghost) has its notes ended at entry: RENDER-26a's "replaces a note
  consumer". A rewired kept consumer is the same (§3.5).
- **A new consumer of a kept scheduler** gets Offs for notes it never got Ons for. `poly` and
  `plugin_events` ignore those, as they ignore a stolen note's Off today.
- **A notes edit that outgrows its scheduler's caps** is an activation with a `grow` lane move. It is
  not a rebuild, and nothing restarts. This is the only place sounding notes are copied
  (`take_state`).

### 8.2 Audio players persist; audio lanes swap

**Today.** The state (taps, fade-in, continuation) is in `Exec::Audio(Box<AudioNode>)`, one per graph
node. The renders are in `LaneSlot::Audio(AudioLane)`.

**After.** They join into one `AudioPlayer` per lane:

```rust
pub(crate) struct AudioPlayer {
    cur: Arc<AudioLane>,
    prev: Option<Arc<AudioLane>>,      // the renders a swap replaced, read by its fading taps
    taps: [Tap; TAPS], ntaps: usize,   // Tap gains `src: u8`: 0 reads `cur`, 1 reads `prev`
    fade_in: Option<u32>, len: u32, prev_moving: bool, cont: u64, dropped: u32,   // as AudioNode
    out: Box<[f32]>,                   // channels × BLOCK: this block's output
}
```

- **One fill per lane.** The player fills `out` once per block before the graph, as schedulers run
  then. `Exec::Audio { lane }` copies it.
- **The same bits.** Two nodes reading one lane computed the same output from the same runs, so a
  single player gives the same bits.
- **A swap** (`LaneSwap::Audio(Arc<AudioLane>)`):
  - while playing: every running tap is pointed at `prev` (`src = 1`), the current material becomes a
    tap reading `prev` (`tap_out`), and `fade_in = Some(0)`. The old renders fade out from their
    continuation and the new ones fade in at the transport, equal-power over 5 ms. That is the same
    declick as a jump (RENDER-9 item 5).
  - stopped: the data is just replaced.
  - the same `Arc` (a cache hit): nothing happens, so a no-op swap is bit-exact.
- **One swap at a time.** A swap while `prev` still has taps waits (§7.1), at most 5 ms. The `prev` that
  is replaced leaves as `Retired::Lane`.
- **Cache pruning.** `audio::Cache::prune` keeps every clip any lane of the new plan plays, moved
  players included, so an edit to one clip of a lane never re-renders its other clips.

### 8.3 What reaches the engine how (the actor's `way`, after slice 2)

| The delta from what plays to the accepted revision | Path |
|---|---|
| empty | nothing |
| notes and control lanes only, each fitting its scheduler | `SwapLane`, as today |
| audio lanes or assets only | the changed clips rendered on the actor thread through the cache, then one `SwapLane` per lane |
| plugin states only (slice 4) | `load_state` in place (§10.2), then `adopt` |
| constant values only | `SetCell` through `value_writes` until §9.3 deletes it, then `Activate` |
| anything else: structure, arrangement, a lane that outgrows its room | `Activate`, prepared with `Basis::Keep` |
| no delta needed: a plugin restart | `Activate` with `identity` |
| the rate changed; or the last activation was refused | `Way::Replace` |
| no player, or it stalled | `Way::Load` |

### 8.4 Arrangement switches (RENDER-27)

A tempo or meter change is an `Activate` of the same structure:

- `identity` continuity, so every kernel, poly, plugin and ring is kept and there are no fades.
- Every notes lane gets a `LaneMove` with new `NotesData`, built in the new tempo map.
- Every audio lane with a warped clip gets a `LaneMove` with new renders.
- Control lanes are by beat, so they move unchanged.

At the block line, all of it switches together:

- the moves, the lane data, and the new `TempoMap` and `Arrangement`;
- the transport keeps its beat, recomputed into the new map;
- each moved scheduler with new data reconciles its sounding notes against it by `(clip, pitch, start
  beat)` at its first block (`sched.rs` `reconcile`). So each pending Off comes from the new data's end
  sample, which is RENDER-26a's "recompute the frame of every pending Off from its beat";
- each audio player crossfades to its new renders (§8.2).

**No kernel can lose a tail on a tempo change.** A mixed change (code plus tempo, PLANE-2) is one
`Activate`, with fades only where the code changed.

### 8.5 Planes, as built after step 7

| Plane | Path |
|---|---|
| Data (notes, control) | `SwapLane`, as today |
| Prepared data: audio lanes, assets | clip renders prepared off the audio thread, then `SwapLane` with a 5 ms crossfade |
| Prepared data: plugin state | `PluginHost::load_state` on the plugin main thread (§10.2) |
| Arrangement | tables and renders prepared off the audio thread, then an `Activate` of the same structure |
| Code | compile, check, verify; `continues`; prepare; `Activate` |

---

## 9. Cells and committed values

### 9.1 Cell carry at entry

At entry (§7.2 step 4), each continued cell whose value changed is applied as a `SetCell` from the old
value to the new would be (RENDER-22a): it ramps over one block if its control is smooth, and steps
otherwise.

- **Top-level cells,** for each `(new, old)` in `Entry.cells`:
  - read the old plan's value and the new plan's value;
  - if the bits differ and the cell is smooth, `write(graph, at, Write::Ramp(old, new))` and push it on
    `ramping` (room for every smooth cell is reserved at prepare), exactly as `Cells::apply` does for a
    slot write (`cells.rs:295`);
  - a non-smooth cell is already a step in its fresh buffer.
- **Voice cells of kept polys,** for each `CellId` in `Entry.voice_cells`:
  - compare the moved voice graph's const value (voice 0's) with the table's value;
  - if they differ, write it into every voice, ramped or stepped by `smooth`, through the same
    voice-cell code `Cells::apply` uses.
  - This is about 1,500 compares at p50 (*estimate*), and only the differing cells are written.
- **Leases.** The server re-aims leases at the new plan's slots under the control lock, in the same
  phase that sends the `Activate`, as today.
  - An update made before the send is in the value `set_cells` used.
  - An update made after the send goes to the new slots, which the new plan drains after `take_over`.
  - So cell carry never overwrites a newer value with an older one.

### 9.2 Kernel wrappers

A kept kernel's `seen` starts at `COPY`, so its first call in the new plan copies each port input once:
the same values unless the edit changed them. Its `Input::Const` inputs are already in its memory
(§5.1).

### 9.3 The value fast path, then its deletion (slice 2's last commit)

**Where it stands at slice 2.** With cell carry, an activation of a value edit sounds exactly like a
`SetCell`, and it keeps every instance. So the server's fast path (`same_layout`, `value_writes`,
`Reach.cells`, the cell-write branch of `Way::Reach`, and `Open::adopt`'s re-key) is no longer
load-bearing for state. Until it is deleted, its fallback cuts nothing.

**The deletion lands only when both hold:**

1. **Measured:** at p50, a committed value through an activation is heard at most 30 ms (p95) after
   the session's request (G7a's harness with a value edit).
2. **The data-plane workflow's faster Tx path doesn't build on the cell-write branch.** Check this
   after rebasing on it.

If either fails, the fast path stays as a pure optimisation, and nothing else depends on the deletion.
The lease rule "the engine installs the committed value in the same step that ends the lease"
(WRITE-22) holds either way, because a renderer built while a lease lives gets its value before it is
sent, as today.

---

## 10. Plugins

### 10.1 Persistent instances

Plugin instances move like every box (§7.2), by name and `Shape::Plugin`. This replaces `CarryKey`,
whose ordinal matching hands an old instance's state to a newly inserted node.

- **Keyed plugins** match by state path in slice 3's second pass (F7).
- **No plugin branch inside voices.** The checker refuses plugins inside `poly` (`check.rs:563`), so
  the runtime's `in_voice` branch is deleted.
- **A changed state blob:**
  - through slice 3, the shape includes the state hash, so the instance is new, exactly as today;
  - slice 4 removes the hash from the shape and loads the state into the kept instance instead.
- **Last-sent values continue.** A kept instance is sent nothing new at a no-op activation.
- **`keys` are never cleared on a load.** An Off for a note the plugin has forgotten is harmless, and
  a rewired consumer must still release what it holds.

### 10.2 `LoadState` (slice 4): a main-thread call, not an engine message

When the delta lists a state change (`GraphDelta.states`, by `module/frames/key`), the actor finds the
instance by state path in `PlanInfo.plugins`. Then:

1. It reads the blob the snapshot verified (`DataSet::state`).
2. `host.load_state(h, blob)` runs on the plugin main thread. It is skipped when the blob's hash equals
   the state the instance *holds* (HOME-6). CLAP's `state.load` is a main-thread call allowed while
   the plugin is active.
3. `inst.reloaded.store(true)`, a new `AtomicBool` on `PluginInstance`. At its next call, the node
   swaps it to false and clears every last-sent value, so every code-owned parameter is sent again in
   that call (HOME-4: code beats the blob). The blob's value plays for less than one block.
4. `host.latency(h)`. A nonzero latency raises the Warning `plugin.latency-uncompensated`, naming the
   plugin. Compensation doesn't exist, and the check refuses latency at default state (RENDER-21), so
   there is nothing to re-plan.

Around it:

- **A state-only change** builds no plan. `reach` gains a `states` list beside its lane swaps, and
  `Plays.revision` is adopted as after a swap.
- **Mixed with a structural change:** the loads run after `prepare` succeeds and before the `Activate`
  is sent.
- **A removed blob:** CLAP has no "load defaults", so the instance is new (its shape's state is `None`
  against `Some`).
- **The host fix,** in `core/plugins/src/inprocess.rs` `load_state` (`:275`). Rename `last_saved` to
  `holds`, and set it on every successful load as well as every save.
  - Without it, save S, then load C, then load S skips the last load.
  - The export's restore depends on exactly that sequence.

### 10.3 Restart (`HostEvent::Restart`)

1. The actor's owner events mark the handle in `restarts`, which replaces `Carry::restart` and
   `PluginInstance.restart`.
2. It queues `save_state(h)`.
3. It settles at once with an identity activation. The marked instance doesn't match (§6.2), so a fresh
   one is created from that saved blob, or from the committed one if the save fails.
4. The fresh one fades in while the old one rings out as a ghost for 5 ms.

Today waits for the next rebuild and loads the blob from disk. A deactivate and activate of the same
instance would need a per-instance hold, and waits for plugin windows.

### 10.4 "loading <plugin>" (RENDER-25, slice 4)

While `prepare` creates plugin instances, the actor publishes `loading: [plugin names]` in the engine
view through the control plane, set before each `create` and cleared after. The web shows "Loading
Surge XT…" in its status area. The old plan plays meanwhile, and the 200 ms budget doesn't count it.

### 10.5 Export: hold, borrow, render, restore, resume (RENDER-1, slice 4)

A song whose export plan has at least one plugin pauses playback. A pure song, or one whose plugins
are all missing, exports beside playback as today.

**`Hold` in the engine** (`core/runtime/src/engine.rs`):

- `Hold(true)` applies once no window runs. For `fade` frames the engine renders normally with its
  output scaled linearly to 0. From the next block on it renders nothing:
  - no kernel, plugin or scheduler runs;
  - the engine clock doesn't move;
  - it outputs silence;
  - `Status.held = true`.
- **The proof of exclusivity.** The callback publishes `Status` after `fill` returns
  (`Callback::publish`). So when the export thread reads `held` (Acquire), the audio thread has
  returned from the last block that called any plugin. It calls none again until it reads `Hold(false)`.
- **While held:**
  - `Activate` waits;
  - `SwapLane` applies, since it touches no plugin, so lane previews keep working;
  - `Transport` commands apply to the frozen transport and take effect at the resume;
  - `Panic` is deferred to the resume;
  - cell writes wait in their slots.
- `Hold(false)`, in order after its `Hold(true)`: the engine resumes from where it held, with its
  output ramped linearly from 0 over `fade` frames. This is a tape pause (owner question Q5).

**The actor** (`crates/server/src/export.rs` `start`, and `Open.exporting`):

```
pure song (no FOp::Plugin in order): as today, a Renderer on its own thread, beside playback.

otherwise:
  actor: exporting = true          settles publish the view but send no Activate and make no host
                                   call on a live instance; swaps and transport go on
  map the accepted revision's plugin nodes to live instances:
      continues(plays.revision, accepted), equal shape -> PlanInfo.plugins' Arc
  export = Render::borrowing(accepted, Basis::Borrow{..})
      kernels fresh (RENDER-1); borrowed nodes flagged reset_first; other plugins created, destroyed after
  player.hold(true); wait up to 2 s for player.held()
      not held (the device stalled): player.hold(false), export with fresh instances, Notice says so
  spawn music2-export, which does:
    for each borrowed h, on the plugin main thread:
        before[h] = save_state(h)                 // the state it has now, to give back
        deactivate(h)
        load_state(h, committed blob)            // when the revision has one; `holds` makes it exact
        set_render_mode(h, Offline)
        activate(h, rate, BLOCK)
    render on this thread: at a reset_first node's first call, host.reset(h)
        (CLAP's reset needs an active instance; code-owned parameters go at the first call, as the
        export plan's last-sent values are None)
    finally (success, error, stop, or a caught panic), a guard dropped on every path:
        for each borrowed h: deactivate(h); set_render_mode(h, Realtime);
                             load_state(h, before[h]); activate(h, rate, BLOCK); inst.reloaded = true
        drop the export renderer (its Arcs go; the live plan still holds the instances)
        player.hold(false)
        post ExportDone: actor exporting = false, settle once
```

**Failures:**

- A failed host call on one instance makes that node fresh for the export, and it goes in the export's
  `problems`.
- A failed restore (`load_state(before)` errors) puts the handle in `restarts`, so the next settle
  replaces it from `before`.
- Quitting during an export takes the same guard path.

**What the producer hears and sees:**

- the output fades out over 5 ms, and the status says `paused: "exporting"`;
- the transport resumes where it was, playing if it was;
- kernel tails resume where they froze;
- plugin voices were reset by the export, which is RENDER-1's stated cost;
- plugins come back with their state intact (the gate).

**CLI.** `m2 render`, `m2 bounce` and agent range renders keep creating their own instances: the CLI
has no live engine.

---

## 11. The Store layout (RENDER-32) and G7f

### 11.1 The code shape: one Store per kernel instance

`KernelInst` owns a `Store<StoreLimits>` with `memory_size(KERNEL_MEMORY)`, `instances(1)`,
`memories(1)` and `tables(1)`. `Graph::process`, `reset`, `prewarm`, `active`, `kernel_memory` and
`PolyNode::process` lose their `store` argument, and each call goes through its own store
(`k.process.call(&mut k.store, n)`).

Why per instance, by elimination:

- **A shared store leaks.** wasmtime frees nothing in a `Store` until it drops (`store.rs:139`), so one
  long-lived store keeps every replaced instance's memory.
- **A shared store can't move:** its instances can't be handed to another plan.
- **A per-batch store** (the open's batch, then each edit's) leaks a replaced instance until its whole
  batch is gone, which for the open's batch may be never.
- **Two `PackageSet`s** (two wasmtime `Engine`s, as in G7d) can only live side by side in separate
  stores.
- **The cost scales with memories, not stores.** Each memory reserves its own 4 GiB plus guards,
  whatever the layout.

### 11.2 G7f: `m2-gates store --reference 50` (`crates/gates/src/store.rs`, new)

The gate runs at engine level, with no server and no device. It builds the kernel population directly
with wasmtime, as `Builder::kernel` does (instantiate, typed funcs, `k_init`, port checks, one prewarm
reset), so the runtime needn't support several layouts.

1. **Count.** Top-level kernel nodes, plus, for each `poly`, voices times its voice graph's kernel
   nodes. It prints the count beside the contract's 6,176. The runtime notes count about 6,060 from
   std's sources, and acceptance gate 7 takes the measured number.
2. **Layouts:**

   | Layout | What |
   |---|---|
   | **S** | one Store for all (HEAD's); the baseline only |
   | **P1** | one per instance, default tunables (4 GiB reservation plus guards) |
   | **P2** | one per instance, `memory_reservation(KERNEL_MEMORY)`, `memory_guard_size(64 KiB)`, `memory_reservation_for_growth(0)`: explicit bounds checks |
   | **P3** | one per instance, the pooling allocator (wasmtime feature `pooling-allocator`, workspace `Cargo.toml`): `total_memories` and `total_core_instances` at 2 × count, `max_memory_size = KERNEL_MEMORY` |
   | **T** | one Store per track, for reference only |

3. **Measured per layout:**
   - build time per instance, split into `Store::new`, `Instance::new`, typed funcs, `k_init` and the
     prewarm `k_reset`;
   - the time to build 1, 14 and 156 instances (an effect, a bass pool, a pluck pool), and all of p50;
   - `task_info` `phys_footprint` before and after, and address space reserved (`mach_vm_region` sum)
     against the process limit;
   - block time: every instance's `k_process(128)` in p50's order, inputs from a fixed-seed noise, over
     11,250 blocks (30 s): mean, p99 and max;
   - drop: all instances on another thread, the longest single drop, and the p99 of a concurrent
     audio-like thread during it, with and without 64-instance batches;
   - the largest count that instantiates, up to 4× p50.
4. **Leak control:** build, replace 10% of the instances 20 times. S's footprint grows; P1's must not.

### 11.3 The decision rule, stated before measuring

**Choose P1, unless one of these fails against S:**

- p99 block time worse by more than 80 µs, which is 3% of a 2.67 ms block;
- footprint worse by more than 64 MB;
- instantiation fails below 2× p50's count, or reserved address space at 2× the count exceeds 25% of
  the process limit;
- p50's full build (opening a project) worse by more than 50%.

**If P1 fails,** choose P3 if it passes all four, then P2 if it passes all four (its bounds checks show
in the block time). If none passes, step 7 stops and the owner decides. S, T and per-batch stores are
never chosen.

**Batched drops** are built only if the chosen layout's mass drop of 6,000 instances raises the
audio-like thread's p99 more than dropping S's single store does. That drop is today's cost on every
edit.

**After slice 2,** G7f's second part plays p50 at 3:00 for 30 s with the arena. It records the p99
callback and an activation's callback time, and writes them into RENDER-32's Today line.

---

## 12. The gates, and how each is measured

Timed gates run in a recorded release run on the reference Mac (M1, 48 kHz, 512-frame device
buffers). The rest run in CI.

**Rig changes** (`crates/gates/src/engine.rs`):

- `Song` carries its `Arc<Revision>`, so the rig calls `continues` exactly as the server does.
- The rig prepares with `Basis`.
- `--no-carry` becomes `--no-arena`: `Basis::Keep` with an empty `Continues`, so everything is fresh
  and the ghosts are the whole old graph. `--keep` already names "keep the work folder". This is the
  negative reference for every arena gate.
- `Probe` gains the last activation's `Stats` (kept, fresh, ghosts, fades), plus `refused` and
  `leaked`.
- `Renderer::output_of(node) -> &[f32]` returns the last block's buffer of a node, looked up through
  `PlanInfo.index`. It is gates-only reading through `Driver::engine()`, with no audio-thread code.

### 12.1 G7a: "In p50, a structural edit at 3:00 is audible within 200 ms after the compile"

- **Server gate:** `m2-gates --server --reference 50 --at 180 add-effect`.
  - `--at` is a new `Opts.at`, default 60; `open()` seeks there before playing.
  - The budget is per reference: 100 ms on p16 (step 3's), 200 ms on p50.
  - Ten edits, alternately adding and removing a `delay` on `t02` through the app's `add_effect` Tx.
- **Pass:**
  - every `heard_ms` ≤ 200;
  - no jump back (`Sampler`);
  - G6's step bound holds across each edit;
  - at most 3 fresh instances per edit, from slice 3 on.
- **Printed:** the parts, from `EditTiming` plus a new `prepared_ns`: the session's request (compile,
  check, verify, not counted), `continues`, prepare, the wait for the block, and one device buffer.
- **Definition.** `heard_ms` counts from `compiled_ns`, taken when the session's request is done,
  check and verify included. The gate is slightly kinder than "after the compile finishes", and
  RENDER-25 says so.
- **Expected:** 20 to 40 ms (*estimate*), against 150 to 170 ms for HEAD's build alone.
- **Engine-rig twin:** `gates::add_effect --reference 50 --start 180` (in place of `one_minute`), which
  keeps measuring what the audio thread pays.
- **Value-edit variant**, for §9.3: the same harness, committing a knob value; p95 ≤ 30 ms after the
  session's request.

### 12.2 G7b: "The tails of untouched tracks are intact"

**The engine-rig gate `tails`** runs p50 on two decks with 128-frame periods, so one fill is one block.

- Deck A plays from 2:55. At 3:00 it prepares and commits `t02` with a `delay` added: G5's edit, which
  is heard.
- Deck B plays the same with no edit.
- After each fill the rig reads each track's export node (the IR's exports, joined to the `Flat` by
  `FNode.name`) with `output_of`.

**Pass (slice 2):**

- each of the 49 untouched tracks' taps is bit-identical between A and B on every block from 2:55 to
  3:10. That covers `t03`'s reverb tail, `t01`'s delay echoes and every voice sounding at 3:00;
- the activation's `Stats` say every stateful node of the 49 tracks and of the mixer was kept;
- `resets` is unchanged and `held_notes` agrees at the end;
- the activation's callback takes at most 1.25× the median callback's wall time.

**Negative control:** `--no-arena`, and HEAD's engine. Both must fail (`t03`'s tap differs right after
3:00). A gate that HEAD passes measures nothing.

**The wrapped instrument (slice 3):** `t02`'s pluck wrapped in `vca(…, 1)`, which is `std:scale`, an
exact multiply by 1.0. It is done twice: through the app's Tx, and as a file save of the wrapped text,
which is the agent's path.

- **Pass:** the whole mix is bit-identical between A and B on every block, fade window included (the
  fade's equality branch, §7.4); `t02`'s pluck is in the kept list.
- **Negative control:** the same edit on slice 2's names restarts the pluck, and must fail.

### 12.3 G7c: "Live playback with no edits is bit-identical to offline"

- **Existing CI tests** in `core/play/tests/bit_identity.rs` (`live_is_the_export_on_every_pure_fixture`,
  `live_is_the_export_on_p16`, `a_no_op_swap_changes_no_bit`) pass unchanged at every slice.
- **New: `a_no_op_activation_changes_no_bit`.** For every pure fixture and p16:
  1. play live from 0;
  2. at 5 s and at 2 later points, mid-note, activate a renderer prepared from the same revision with
     `Basis::Keep` and `identity`;
  3. the whole output must equal offline bit for bit, and `Stats` must report everything kept, with 0
     fresh, 0 ghosts and 0 fades.

  This covers schedulers, voice pools, rings, audio players, cell carry and the wrappers' first-call
  copies.
- **New variants:**
  - every lane's data rebuilt (new `Arc`s, equal contents) and sent in the lane moves;
  - `an_identity_arrangement_activation_changes_no_bit`.
- **Recorded:** `live_is_the_export_on_p50` and `a_no_op_activation_on_p50` (`--ignored`).
- **Offline byte identity:** `core/session/tests/renders.json`. Every pure fixture's offline render
  stays byte-identical at every slice.

### 12.4 G7d: "A kernel module rebuilt with the same kernel id but a new hash starts from init"

The runtime test `a_rebuilt_kernel_module_starts_from_init`, in
`core/runtime/src/tests/live/keep.rs`:

1. Load std into `PackageSet` A.
2. Copy std to a temp folder and append a custom section (id 0, name `m2-test`) to `std.wasm`. That is
   valid wasm with the same code and a new SHA-256: `PackageSet` B.
3. Play a one-note song through `std:delay` with feedback. Mid-echo, activate the same `Flat` prepared
   with B and `Basis::Keep`.

**Pass:** `Stats` reports 0 std kernels kept; after the window the echo is gone, because the new delay
starts empty; and the output differs from never activating by more than −60 dBFS.

**Controls:**

- `PackageSet` C, loaded from the original bytes (a new `Module`, the same hash), keeps every instance,
  and its output is bit-identical to never activating. This proves the shape compares content, not
  objects.
- `a_wrong_shape_move_is_refused`: a hand-made `Entry` with one wrong shape hash is refused whole,
  `refused` counts 1, and the old plan plays on, bit-identical.

The test is runtime-level because the app loads packages once per session.

### 12.5 G7e: "An export of `examples/with-plugins` pauses playback, reuses its instances, and playback resumes with their state intact"

**The server gate `m2-gates --server export`** is skipped without Surge XT and Airwindows on
`CLAP_PATH`. It plays from 0:02, then exports through `m2_server::export::start`.

- **Pauses:** `held` before the first host call; `paused: "exporting"` published; the engine clock
  doesn't move.
- **Reuses:** `created()` is unchanged, and the plan's handles before and after are the same.
- **State intact:** `save_state` bytes before and after are equal.
- **Resumes:** playing at the same transport frame within two buffers of `done`, with no step over G6's
  bound at the resume.
- **The file:** a WAV of the song's length. Plugins aren't bit-exact across instances, so its RMS per
  second and onsets are compared with `m2 render`'s, within 1 dB.

**The CI test `an_export_borrows_and_gives_back_the_live_instances`** (`crates/server/src/graph/tests.rs`)
uses the probe plugin (`core/plugins/src/probe.rs`, `clap:m2.probe`). The project has a saved state,
and code sets a gain, so the in-memory state differs from the blob.

- The main-thread call log is, in this order: `save_state`, `deactivate`, `load_state` (committed),
  `set_render_mode(Offline)`, `activate`; then after the render `deactivate`,
  `set_render_mode(Realtime)`, `load_state` (saved), `activate`.
- `reset` is called on the export thread after the first `activate`.
- No `process` call comes from the audio thread between the first `deactivate` and the last
  `activate`.
- The borrowed export equals a fresh-instance export bit for bit. The probe is deterministic, so this
  proves `reset` plus `load_state` give RENDER-1's exact start.
- Negative control: with the hold removed (a test-only flag), the probe sees audio-thread `process`
  calls inside the window.

**A pure song** (`examples/first-song`) exports beside playback: no hold, no state call.

### 12.6 G7f: "The per-instance Store cost at 6,176 instances is measured, and the store layout is chosen from it"

See §11. The report prints the measured count, the table of layouts against metrics, the rule's
outcome, and, after slice 2, the arena's p99 at p50.

### 12.7 The churn test (no memory growth)

`random_structural_edits_leak_nothing`, in `core/play/tests/live.rs`, is a CI test on first-song:

- 50 structural edits while playing (wraps, unwraps and replacements on rotating nodes), plus 5 tempo
  changes;
- after draining the dropper: `LIVE_KERNELS` and the host's `live()` equal their values before the run,
  and `Engine.leaked == 0`.

A recorded p16 run (200 edits) also prints the footprint, and must stay within +20 MB.

### 12.8 Gates that keep passing

- Every step 3 gate: G1 to G7, G10, G11, G8 `previews` and G9 `drag`.
- With-plugins: its "carried instances" checks become "kept by the arena" with the same assertions
  (`same_instances` in `crates/gates/src/plugins.rs`: `created()` unchanged, notes go on).
- `core/play/tests/no_alloc.rs`, extended (§13), and `no_blocking.rs`.
- The step 6 Tx gates, with their names updated in slice 3.

---

## 13. Tests, by crate (each names its rule in a comment)

**`core/runtime/src/tests/live/keep.rs`** replaces `carry.rs`:

- RENDER-25:
  - `an_activation_moves_every_instance_that_continues`;
  - `a_changed_shape_starts_from_init` (a channel count, a kernel id, a voice graph);
  - `a_rebuilt_kernel_module_starts_from_init` with its identical-bytes control (G7d);
  - `a_wrong_shape_move_is_refused`, `an_activation_against_another_plan_is_refused`;
  - `a_failed_prepare_leaves_what_plays_untouched`;
  - `a_plan_that_went_non_finite_keeps_no_kernel`, `a_trapped_plan_recovers_at_the_next_activation`
    (§7.7).
- RENDER-25a:
  - `an_inserted_effect_fades_dry_to_wet`, which checks the window's samples against `o + (n − o)·g`;
  - `a_removed_effect_rings_out_as_a_ghost_then_goes`, where `Retired::Nodes` holds it at the
    window's end;
  - `a_silent_edit_changes_no_bit`, the equality branch;
  - `an_effect_reorder_replaces_the_cycle_and_keeps_the_rest`;
  - `a_ghost_reading_a_removed_control_lane_reads_its_ghost_lane`;
  - `feedback_rings_survive_an_activation`, with no 128-sample hole.
- RENDER-26a:
  - `a_replaced_instrument_releases_its_notes`;
  - `a_kept_consumer_keeps_its_notes` (replaces `a_carried_instance_keeps_its_notes`);
  - `a_rewired_poly_is_kept_and_releases_its_notes` and `a_rewired_plugin_lets_go_of_its_keys` (F8);
  - `a_lane_that_outgrows_its_room_grows_without_a_restart`;
  - `note_ids_never_repeat_into_a_plugin` (replaces `ids_never_repeat_after_a_renderer_without_notes`);
  - ported with the same assertions: `a_fresh_instance_chases_nothing`,
    `a_lane_that_shrank_lets_go_of_what_doesnt_fit`, `notes_the_edit_removed_are_let_go`.
- RENDER-27:
  - `a_tempo_change_keeps_every_kernel_and_the_beat`;
  - `a_tempo_change_recomputes_pending_offs`;
  - `an_audio_clip_edit_crossfades_and_keeps_the_rest` (on `examples/resample`).
- RENDER-23 and RENDER-22a:
  - `a_committed_smooth_value_ramps_through_an_activation`;
  - `a_kept_polys_voice_cells_take_the_new_values` (a write counter: only the differing ones);
  - `a_lease_value_set_before_the_send_plays_from_the_first_block`.
- RENDER-28: `activations_are_never_coalesced`.
- Slice 3 (F7, ID-8): `keyed_instances_follow_their_keys`, `a_moved_keyed_plugin_continues`.
- Panic: `panic_during_a_window_resets_ghosts_and_ends_on_time`.

**`core/runtime/src/sched.rs` and `audio.rs`:**

- `take_state_keeps_sounding_notes`;
- `an_audio_swap_crossfades_without_a_step` (a G6-style bound);
- `a_no_op_audio_swap_changes_no_bit`;
- `one_player_per_lane_gives_the_old_bits` (two nodes on one lane, against two `AudioNode`s);
- the existing swap, wrap and ordinal tests unchanged;
- the adoption and `release_all` tests deleted with what they test.

**`core/play/tests`:**

- `bit_identity.rs`: G7c's additions.
- `no_alloc.rs`: one activation under the allocation trap, with cell writes from another thread, that:
  - moves kernels, polys, two probe plugins, rings and audio players;
  - grows a lane;
  - makes ghosts, ghost lanes and fade nodes;
  - releases notes;
  - ends a window and retires the leftovers;
  - refuses a stale activation;
  - swaps an audio lane mid-window;
  - panics mid-window;
  - holds and resumes, with a deferred panic.

  It also asserts that `kernel_memory_bytes` is unchanged for the kept instances.
- `live.rs`:
  - `no_edit_or_jump_leaves_a_note_stuck` and `random_edits_and_jumps_never_strand_a_gate`, extended
    with random structural edits between jumps;
  - the churn test (§12.7);
  - `a_hold_freezes_and_resumes_without_a_step`.

**`core/session`:**

- `continues` (slice 2): `an_insertion_above_continues_everything` (step 6's gate edit),
  `a_label_rename_continues_everything` (ID-10), `untouched_files_continue_whatever_changed`.
- `continues` (slice 3): `a_loop_that_grows_resets_its_occurrences` and
  `a_keyed_loop_keeps_through_reorder_and_growth` (ID-14), `a_moved_keyed_plugin_continues` and
  `adding_a_plugin_key_continues_its_instance` (ID-8).
- `text.rs`:
  - `token_hunks_are_an_edit_script`, a property test: applied to old the hunks give new; they are
    sorted, disjoint and on character boundaries; carried bytes are equal;
  - `two_literals_on_adjacent_lines_are_two_hunks` (F1);
  - `a_reformat_changes_no_token`.
- `delta_tests.rs`, all through `delta_between`:
  - `a_wrap_by_a_file_write_keeps_the_wrapped_names`, on p50's `t02` text, the nested `reverb(pad(…))`
    → `reverb(delay(pad(…)))`, and the unwrap (F2);
  - `adjacent_value_edits_are_values`;
  - `a_component_swap_at_one_call_is_new_and_an_alias_is_not` (F3, PKG-9);
  - `names_ignore_everything_above_a_key_frame` (F6);
  - `old_side_names_are_own_offsets`;
  - `delta_names_are_unique_on_every_fixture`, and the two-process hash test, kept with their expected
    names updated.

**`core/script`:** `every_call_has_an_open_paren`: on every fixture and std file, `Site.open` points at
a `(` within `start..end`.

**`core/plugins`:**

- `load_then_restore_is_never_skipped`: save S, load C, load S, and S loads;
- `a_load_of_the_held_state_is_skipped` (HOME-6);
- `a_state_load_resends_code_owned_parameters` (HOME-4).

**`crates/server/src/graph/tests.rs`:**

- `a_state_change_loads_into_the_running_instance` (no new instance; `created()` unchanged);
- the G7e probe test;
- `settles_during_an_export_wait_then_apply_once`;
- `lane_previews_play_during_an_export`;
- `a_tempo_change_keeps_every_instance`;
- `a_restart_request_replaces_the_instance_from_its_saved_state`;
- `loading_is_published_while_a_plugin_is_created`;
- `a_refused_activation_recovers_by_replace`;
- `a_failed_build_then_a_lane_edit_rebuilds`, kept;
- the lease tests, kept.

**Existing tests whose meaning changes:**

| Test | Change | Rule |
|---|---|---|
| `activations_crossfade_without_jumping_back` | the fade is per edge; still no jump back | RENDER-25a, RENDER-8 |
| `activations_while_a_voice_sounds_dont_click` | a kept voice now keeps sounding, where today it was released; the click check is unchanged | RENDER-25 |
| `structural_messages_keep_their_order` | consecutive activations no longer coalesce | RENDER-28 |
| `a_changed_state_is_a_new_instance_and_the_old_one_goes_with_its_renderer` | unchanged until slice 4, then "a changed state loads into the same instance" | RENDER-26b |
| `a_failed_build_leaves_its_carry_whole` | becomes `a_failed_prepare_leaves_what_plays_untouched` | RENDER-25 |
| gates `--no-carry` | becomes `--no-arena`, the negative reference for G7b | RENDER-25 |

---

## 14. Build order: four slices, each landing green

**Coordination.** The data-plane workflow is editing `core/session`, `crates/server`, `crates/gates`
and `web` now. Slice 1's runtime and play work touches none of them and can start at once. G7f is a new
file in `crates/gates` plus one registration line, and lands after that workflow. Slices 2 to 4 rebase
on it. Slice 3 can run beside slice 2's second half; the two meet only in `continues.rs`.

### Slice 1: movable instances, and the Store measured (about 1.5 weeks). Nothing audible changes.

Commits, in order:

1. **`KernelInst` with its own `Store`.** `KernelNode` splits into a wrapper (`inputs`, `seen`) and
   `inst: Option<Box<KernelInst>>`. The `store` argument goes from every `Graph` function. Add
   `LIVE_KERNELS`.
2. **`PolyNode`** splits into a wrapper and `pool: Option<Box<VoicePool>>`. `Exec::Capture(PortRef)`
   becomes `Exec::Capture(u16)` into `PolyNode.captures`, and the voice `Graph::process` takes
   `(outer, captures)`.
3. **`AudioPlayer` in `LaneSlot::Audio`.** It is `AudioNode`'s state plus the renders, filled once per
   lane per block. `Exec::Audio { lane }` copies it.
4. **Engine-wide note ids.** `Renderer.next_note` is passed to `scheduler.run`, and `enter_from` copies
   it. Delete `NoteScheduler.next_id`, `next_id()`, `raise_next_id`, `PluginInstance.next_id`,
   `held_cap` and `hold`; `carry_from` stops raising ids.
5. **`Plan` and `Lanes` inside `Renderer`** (§4.2), with `plan.rs` holding `Shape`, `hash64`,
   `PlanInfo` and `PackageSet::module_sha`. `prepare(.., Basis::Fresh)` is the only basis.
   `build_live` stays as a wrapper that still takes the `InstanceSource`.
6. **G7f, part 1** (`crates/gates/src/store.rs`), once `crates/gates` is free. The layout is decided by
   §11.3's rule. If P1 fails, the engine configuration changes in `packages::engine()` now, before
   anything depends on it.

**Lands green because:**

- every existing test passes unchanged: `bit_identity`, `no_alloc`, `live.rs`, `carry.rs`, `cell.rs`,
  `tests.rs` and the gates;
- offline renders are byte-identical (`renders.json`);
- `one_player_per_lane_gives_the_old_bits` passes;
- `prewarming_keeps_kernel_memory_flat` and `kernel_memory_bytes` hold with per-instance stores.

Size *estimate*: +700 / −400 lines in `core/runtime`, about 40 in `core/play` tests, about 350 for
G7f.

### Slice 2: the arena (about 2.5 weeks)

- **`core/runtime/src/plan.rs`** (about 800 lines):
  - `Basis::Keep`, the matching, edges, ghosts, ghost lanes, fade nodes and Kahn order, with the cycle
    rule;
  - `Entry`, `Stats`, `take_over` (verify, then move), `end_fade`, `take_from` per kind;
  - `Exec::Fade`, cell carry, the `wipe` basis and the taint flag.
- **`engine.rs`:**
  - `try_apply` calls `take_over`;
  - delete `Fade`, `fading`, the two-renderer mix in `render`, the coalescing branch, `heard[1]`, and
    the fading arms of `apply_now` and `resets()`;
  - add `end_fade` at block lines, `Retired::Nodes`, `refused_activations`, `leaked`, and
    `Applied.refused`.
- **`sched.rs`:** delete `adopt`, `adopted`, `released` and `release_all`; add `take_state`.
- **`audio.rs`:** `AudioPlayer::swap` and `Tap.src`; the `Jump::Enter` arm stops clearing `fade_in`.
  `LaneSwap::Audio` is added.
- **`plugins.rs`:**
  - delete `CarryKey`, `Carry`, `InstanceSource`, `Fresh`, `carry_from` and `lend`;
  - `PluginInstance` keeps `host`, `h`, `rate` and its `Drop`, and gains `shape`;
  - `PluginNode.lent`, `PluginView.lent`, `render_block_lent` and `render_block`'s `release_all` go;
  - so do the runtime's `in_voice` plugin branch and `Builder.{ordinals, source}`.
- **`core/session`:** `tx/continues.rs` (§3.2) and the exposed `ids` in `delta.rs`, about 80 lines.
- **`crates/server/src/graph/actor.rs`:**
  - `Plays.info: Arc<PlanInfo>` replaces `slots`, `caps`, `cell_slots` and `cells`;
  - `Open.carry` goes, and `Open.restarts` arrives;
  - `reach` calls `continues` and keeps the delta;
  - audio lanes and assets are rendered, then swapped;
  - `way` follows §8.3, and the `Way` variants lose their `Carry`;
  - the taint check.
- **`crates/gates`:** the rig on `Basis`, `--no-arena`, `Probe` stats, `output_of`, the `tails` gate
  (G7b), `--at` and `--start` (G7a), and G7f part 2.
- **Tests:** `keep.rs`, G7c's and G7d's tests, the extended `no_alloc.rs`, the churn test.
- **Last commit, conditional (§9.3):** delete `same_layout`, `value_writes`, `Reach.cells`, the
  cell-write branch of `Way::Reach`, and `Open::adopt`'s layout re-key.
- **Contract edits that land with it:** Planes table, RENDER-10, 16, 22 Today, 23, 25, 25a, 26, 26a,
  27, 28, 32 Today, the Engine shape, and HOST-5's first half.

**Lands green because:**

- G7a, G7b (untouched tracks), G7c, G7d and G7f part 2 pass;
- every step 3 gate passes at p16 and p50;
- with-plugins passes with "kept" in place of "carried";
- `a_failed_build_then_a_lane_edit_rebuilds` and the lease tests pass on the new `Plays`.

Size *estimate*: +1,500 / −900 lines.

### Slice 3: names that survive edits (about 1 week)

- F1, F2, F3, F5, F6 and F7 (§3.3), in `core/session/src/text.rs`, `core/script/src/sites.rs`,
  `core/session/src/tx/delta.rs` and `continues.rs`.
- G7b's wrapped-instrument variant, through the Tx and as a file save.
- Optional: the editor's text carry onto `Edits::between`, and the deletion of `m2_ir::control_id`.
- **Contract edits that land with it:** ID-7, ID-13, ID-14, ID-15, WRITE-8, WRITE-12, WRITE-22 and
  DIAG-3.

**Lands green because:**

- step 6's tests pass, including `delta_names_are_unique_on_every_fixture` with its new spelling and
  the two-process hash test;
- the expectation tests are unchanged, since they judge by spans;
- the new `continues`, `text.rs` and delta tests pass.

Size *estimate*: +300 / −120 lines.

### Slice 4: plugins persist (about 1.5 weeks)

- **`LoadState`:** the actor's `states` path, `PluginInstance.reloaded`, the latency warning,
  `Shape::Plugin` losing `state`, and the `holds` host fix.
- **Restart from saved state** (§10.3).
- **Export:** `Msg::Hold`, `Status.held`, `Player::hold` and `held`, `Basis::Borrow` and
  `reset_first`, the export thread's protocol and its guard, `Open.exporting`, and `ExportDone`.
- **Status:** "loading <plugin>" and "paused for export" in the engine view and the web's status area.
- **G7e:** the server gate and the probe CI test.
- **A debug-build allocation counter in `apps/music2-desktop`,** which RENDER-28 asks for: the counting
  allocator from `no_alloc.rs`, made a module of `m2-play`. It logs once a second while its count is
  nonzero, and never panics in the callback.
- **Contract edits that land with it:** RENDER-1, RENDER-26b, HOST-5's second half, HOST-6 Today,
  HOME-4 Today, HOME-6, the `PluginHost::load_state` doc, and acceptance gate 7's count.

**Lands green because** G7e passes, with-plugins G5 and G7 pass, the `core/plugins` tests pass, and
`main_thread.rs` passes with the probe.

Size *estimate*: +600 / −100 lines.

**In all,** about 6.5 weeks of work, about 6 elapsed with slice 3 overlapping, against the order of
work's 4 to 6.

---

## 15. What this deletes

| Deleted | Where | Replaced by |
|---|---|---|
| `CarryKey` (with its ordinal and state hash), `Carry` (`none`, `of`, `source`, `handles`, `restart`), `InstanceSource`, `Fresh` | `core/runtime/src/plugins.rs` | `continues`, `Shape` and moves; `Basis` |
| `carry_from`, `lend`, `Renderer::carry_from`, `render_block_lent` | `plugins.rs`, `lib.rs` | `take_over` |
| `PluginInstance.{key, restart, held_cap, next_id}`, `key()`, `hold()`, `mark_restart()` | `plugins.rs` | `Shape::Plugin`, the actor's `restarts`, engine-wide ids |
| `PluginNode.lent`, `PluginView.lent` | `lib.rs` | nothing: no instance is processed twice |
| `Builder.ordinals`, `Builder.source`, the `in_voice` plugin branch (`ordinal = u32::MAX`) | `lib.rs` | moves; the checker already refuses plugins in `poly` |
| `Renderer::build_with`, `build_live`, the per-renderer `Store` field | `lib.rs` | `Renderer::prepare(.., Basis)`; a `Store` per `KernelInst` |
| the `store` parameter on `Graph::{process, reset, prewarm, active, kernel_memory}`, `PolyNode::process`, `KernelNode::{process, reset}` | `lib.rs` | each instance's own store |
| `Exec::Capture(PortRef)` with outer indices inside voices | `lib.rs` | `Exec::Capture(u16)` into `PolyNode.captures` |
| `Exec::Audio(Box<AudioNode>)`, `AudioNode` as a graph node, and its "nothing is faded at a renderer's first block" case | `lib.rs`, `audio.rs` | `AudioPlayer` in `LaneSlot::Audio`; `Exec::Audio { lane }` |
| `render_block`'s `release_all` parameter | `lib.rs` | persistent schedulers; `Entry.release` |
| `NoteScheduler::{adopt, raise_next_id, next_id}`; fields `next_id`, `released`, `adopted`; `run`'s `release_all` | `core/runtime/src/sched.rs` | persistent schedulers; `Renderer.next_note`; `take_state` for growth |
| `struct Fade`, `Engine.fading`, `fading()`, the two-renderer mix, `heard[1]`, the fading arms of `apply_now` and `resets()`, the coalescing branch | `core/runtime/src/engine.rs` | `Exec::Fade` and ghosts inside one plan |
| `Open.carry`, the `Carry` in `Way::{Activate, Replace, Load}`, `build()`'s `InstanceSource`, `Carry::restart` in `heard_plugins` | `crates/server/src/graph/actor.rs` | `Plays.info`, `Basis`, `Open.restarts` |
| `Plays.{slots, caps, cell_slots, cells}` | `actor.rs` | `PlanInfo` |
| `reach`'s refusal of audio lanes and assets (a rebuild today) | `actor.rs` | rendered, then `SwapLane` |
| (§9.3, conditional) `same_layout`, `value_writes`, `Reach.cells`, the cell-write branch of `Way::Reach`, `Open::adopt`'s layout and `CellMap` re-key | `actor.rs` | cell carry inside an activation |
| `Lab.carry` (`--no-carry`), the rig's `Carry` and `carry_epoch` | `crates/gates/src/{lib,engine,main}.rs` | `--no-arena`, `Basis` |
| `tests/live/carry.rs`; `sched.rs`'s adoption and `release_all` tests | `core/runtime` | `tests/live/keep.rs` |
| (slice 3, optional) `carry_text`, `carry_ends`, `carry_lines` | `crates/editor/src/controls.rs` | `text::Edits::between(..).carry` |
| (slice 3) `m2_ir::control_id`, which only a test uses | `core/ir/src/lib.rs` | the control constant's delta name |
| Contract concepts: ID-13's numbered `SiteId(u32)` and its rule 4 (to Deferred); the engine shape's `arena: Map<RuntimeId, Slot>`; the `SetArrangement` and `LoadState` messages; RENDER-28's coalescing; RENDER-32's per-partition fallback; step 3b's clarification C7 | `docs/architecture.md` | §16 |

**Unified:**

- **One `Activate` with moves** carries: a code edit, a tempo change, a notes lane that outgrows its
  room, a plugin restart, a package change and (after §9.3) a committed value. Each one also heals a
  tainted plan.
- **One `SwapLane`** carries notes, control and audio lanes and assets, with one address,
  `(generation, slot)`.
- **One way to move:** kernels, voice pools, plugins and rings move by `take_from`, and lanes by
  `LaneMove`.
- **One mechanism** gives a removed node's ring-out and a replaced note consumer's release: a ghost.
- **One flag** serves `LoadState` and export's restore: `load_state` plus `reloaded`.
- **Export's reuse is activation's match,** sharing `Arc`s instead of moving boxes.
- **One activation fade mechanism** (edge fades). `LiveFade` in `core/play` stays only for replacing a
  whole engine: opening a project, a rate change, a refused activation's recovery, closing.

**Kept on purpose:**

- `LiveFade`, and `Renderer::enter_from` (it serves `take_over` and `Replace { keep: Position }`);
- `Player::send`'s 500 ms `Stalled`;
- `Renderer::seek`'s offline replay (RENDER-1);
- `Jump::Enter`;
- the `mem::forget` fallback, now counted;
- the server's value fast path, until §9.3's two conditions hold.

---

## 16. Contract edits

Each lands in the slice that changes the code (the contract's own rule: "A change that breaks a rule
changes this page first, in the same commit").

| Rule | Now (in short) | Proposed (in short) | Why | Slice |
|---|---|---|---|---|
| Planes table: Prepared data and Arrangement rows, Code row | prepared data: `SwapLane`, `LoadState`; arrangement: `SetArrangement`; code: "new plan; Activate" | audio lanes and assets: renders prepared off the audio thread, then `SwapLane`, crossfaded over 5 ms; plugin state: `PluginHost::load_state` on the plugin main thread; arrangement: tables and renders prepared off the audio thread, then an `Activate` of the same structure that keeps every instance; code: "…; prepare against the plan that plays; `Activate`" | an activation that keeps everything is a data switch: one path, one test | 2 (state: 4) |
| RENDER-1 | calls: `deactivate`, `reset`, `load_state`, `set_render_mode(Offline)`, `activate` | "The engine is held first: it fades out over 5 ms and processes nothing, which it publishes before any host call. Each reused instance: `save_state`, `deactivate`, `load_state` of the committed blob, `set_render_mode(Offline)`, `activate`, then `reset` on the render thread before its first `process`. Afterwards `deactivate`, `set_render_mode(Realtime)`, `load_state` of the saved state, `activate`, and the engine resumes where it held, fading in over 5 ms." Today: the `Hold` | CLAP's `reset` needs an active instance; "the state they had" needs a save first; the hold makes exclusivity provable | 4 |
| RENDER-10 | as is | add: "A plan activation never resets a kept instance." | states the arena's guarantee | 2 |
| RENDER-16 | "A voice's RuntimeId is its allocator slot." | "A `poly` is kept or replaced whole; a kept pool's voices keep their slots, which are their RuntimeIds." | 97% of instances are voices; one box keeps entry O(top level) | 2 |
| RENDER-22 Today | the server's guess (`same_layout`) decides cell write or new renderer | "A committed value reaches the engine as a cell write, or in an activation that keeps every instance and applies each changed continued cell as a `SetCell` from its old value would be." After §9.3: "in an activation, …" | as built | 2 |
| RENDER-23 Today | "cells live and die with their renderer until the arena keeps them across plans (step 7)" | "Cells stay per plan. Leases are re-aimed at each activation; a continued cell whose value changed is applied at activation as a `SetCell` would be (RENDER-22a)." | cells never needed to persist | 2 |
| RENDER-25 | "keeps every instance whose RuntimeId and shape match"; shape listed; "after the compile" | add: "Matched when the plan is prepared, off the audio thread, against the plan the engine will be playing; `Activate` carries the moves and names that plan. The audio thread checks that plan's id and each move's shape before moving anything, and refuses the whole activation on a mismatch. A kernel's module hash is the SHA-256 of its package's wasm module, which also fixes its state ABI; in ABI v0 its only setting is its channel count. A `poly`'s shape is its voices, channels and voice-graph structure without cell values. A plan that trapped, or put a non-finite sample at the output, keeps no kernel, voice or ring state at its next activation." The 200 ms budget counts "after the session's request (compile, check, verify)". Today rewritten, including the stale retired-queue text | where matching runs, what makes stale memory impossible, and what the gate measures | 2 |
| RENDER-25a | "old and new instances run together for one fade window … at least at every module output whose subgraph changed … Step 3 … crossfades the whole output" | "An activation crossfades, linearly over 5 ms, every input of a kept node whose source changed, and the song output when its source changed. A replaced or removed node runs on in the new plan for that window as a ghost, its notes ended, reading what it read, then is retired. Nothing else fades; equal signals stay bit-identical. An edit that would make that window's graph cyclic (a reorder of kept nodes) replaces the nodes on the cycle." | fading at module outputs needs kept instances processed twice (lending); edges need neither | 2 |
| RENDER-26 | "replace one lane's data in the running nodes"; addressed by renderer generation | add: "Audio lanes and assets swap too: their clip renders are prepared off the audio thread, and the old renders crossfade out over 5 ms as at a jump. `SwapLane` names the plan and the lane's slot in it; a swap for another plan is refused." | one data path for every lane kind | 2 |
| RENDER-26a | "An activation that replaces a note consumer first sends an Off…"; `SetArrangement` recomputes Offs; Today: adoption, C7 pending | "Each notes lane's scheduler persists across plans with its sounding notes, keyed by `LaneAddr`, and note ids are unique across the engine. An activation that replaces a note consumer, or keeps one whose notes input now reads another lane, first ends every note sounding into it. An arrangement switch reconciles every scheduler with its new data, which recomputes each pending Off from its beat." Delete C7's paragraph; the clarification is withdrawn | adoption existed only because schedulers died with their renderer | 2 |
| RENDER-26b | "MUST reach the running instance as `LoadState` … a plan-only activation re-plans the compensating delays" | "…as `load_state` on the plugin main thread, into the instance that plays, skipped when its hash equals the state the instance holds (HOME-6); its code-owned parameters are sent again at its next block (HOME-4). Until compensation ships, a latency that becomes nonzero raises `plugin.latency-uncompensated` and plays uncompensated." | CLAP's state load is a main-thread call; there is nothing to re-plan while RENDER-21 refuses latency | 4 |
| RENDER-27 | "…then switch them together at one block boundary" | add: "by an `Activate` of the same structure, which keeps every instance; an audio-lane change takes `SwapLane` with its prepared renders" | the same path as code; a tempo change can't cut a tail | 2 |
| RENDER-28 | messages `(Activate, SwapLane, SetArrangement, LoadState, Transport)`; "the sender waits … or coalesces to the newest plan" | messages `(Activate, SwapLane, Transport, Panic, Hold)`. "Activations are never coalesced: each applies after the previous one's fade window, against the plan it names. A sender waits up to 500 ms, then reports the player stalled; the next change opens a new player." Today: fix the first sentences, and "debug app builds count audio-thread allocations and log them (step 7)" | move lists are relative to one plan; the stall bound is what's built | 2 (debug count: 4) |
| Engine shape (step 7) | `Plan`, `DataSet`, `Engine { arena: Map<RuntimeId, Slot>, … }`, six messages | `Renderer { plan: Plan { graph, order_after, cells, out, entry, info }, lanes: Lanes, tempo, arrangement, transport, clock, next_note }`; `Engine { current: Renderer, clock, … }`; `Msg { Activate(Box<Renderer>), SwapLane(generation, slot, LaneSwap), Transport(Cmd), Panic, Hold(bool) }`. "The arena is the boxes in the playing plan's stateful nodes; they move at activation by a move list computed off the audio thread. No RuntimeId reaches the audio thread." | what is built | 2 |
| RENDER-32 | "One `Store` per instance, measured first at 6,176. If that costs too much, one `Store` per partition." | "One `Store` per kernel instance, as G7f measured at p50's count (recorded here). If that costs too much: wasmtime's pooling allocator, or a smaller memory reservation. Never a `Store` shared across plans, which keeps every replaced instance's memory until it drops." | partitions don't exist until 8a, and a shared store leaks | 1 (Today: 2) |
| ID-7 | "A frame … pins SiteIds from step 7." | "A `key()` frame is named by its key in every identity below it; nothing positional above the innermost one is part of an InstancePath." | what pinning means, as built (F6) | 3 |
| ID-13 | "the session MUST give each site a `SiteId(u32)` when it accepts a code revision", rules 1 to 4 | "A site's identity across two revisions is its file and the point of its argument list's `(`, in the reference coordinates of the comparison (WRITE-8). Nothing numbers sites. A Tx is judged in its own edits' coordinates; every other comparison, the engine's included, uses `Edits::between`: a line diff whose hunks a token-level Myers diff refines. A `key()` frame is named by its key. An unmatched or ambiguous site is new, and its state resets." Rule 4 moves to the Deferred row "Keyed iteration helpers and better structural matching" | a counter is session history and would break step 6's two-process hash gate; the engine only compares what plays with what is accepted | 3 |
| ID-14 | as is | add: "Applied when the engine matches instances; a `key()` frame has no occurrence." | built in `continues` (F5) | 3 |
| ID-15 | "RuntimeId = InstancePath / (the site of the `kernel()` or `plugin()` call …)" | "…of the `kernel()`, `plugin()`, `poly()` or `feedback()` call. Segments are `key:<k>` or `<ComponentId>@<file>:<point>#<occurrence>`. A keyed plugin no RuntimeId matches is matched by its state path." | poly and feedback hold state too (F3, F7) | 3 |
| WRITE-8 | "each frame a call site given as its byte range … From step 7, SiteIds replace the mapped ranges"; the delta drives `LoadState`, `SetArrangement` | "each frame its ComponentId and its call's point (ID-13)". The delta drives: values → `SetCell` or an activation; lanes, audio lanes and assets → `SwapLane` (audio with prepared renders); states → `load_state`; the arrangement → prepared tables and an `Activate`; structure → `continues`, prepare and `Activate`. Delete the last sentence | as built; folds in step 6's C17 | 3 |
| WRITE-12, WRITE-22 | "carried by the hunks of a line diff" | "carried by `Edits::between` (ID-13)" | one correspondence | 3 |
| HOST-5 | "From step 3 … matched by module, plugin id, ordinal and state hash. From step 7 they persist in the arena by RuntimeId." | "Plugin instances persist across plans like every instance (RENDER-25): by RuntimeId and shape (id, version, channels), a keyed one also by its state path. A state change is loaded into the instance (RENDER-26b). An instance that asked for a restart is new at the next activation, created from its saved state. Offline renders in the app share the live instances (RENDER-1); the CLI creates its own." | the ordinal handed one instance's state to another on insertion | 2 (state and export: 4) |
| HOST-6, HOME-4 Today | stale (checked in `understand-contract.md` §4) | rewritten to the code | the contract's own rule | 4 |
| HOME-6 and the `PluginHost::load_state` doc | "skipped when its hash equals the state the instance last saved" | "…equals the state the instance holds: the one it last saved or last loaded" | otherwise the export's restore is skipped | 4 |
| DIAG-3 | "from step 7 the notice also compares SiteIds" | "…compares the `(` point of each module's `track()` call (ID-13)", or stays deferred | there are no SiteId numbers | 3 |
| Deferred: "Sending deltas to clients" | "With step 7, or when UI update latency is measured over budget" | "When UI update latency is measured over budget" | a UI concern; it would only enlarge step 7 | 2 |
| Deferred: "Plugin windows, …" | trigger: "The host process exists, and the arena keeps instances alive across plans" | "The host process exists" | the arena exists | 4 |
| Acceptance gate 7 | "6,176 instances" | the count G7f measures (about 6,060 by std's sources) | the gate states what it measured | 1 |
| Stale Today lines | RENDER-6, 9, 12 to 14, 17, 25, 25a, 27, 28, 33, HOST-6, HOME-4 (`understand-contract.md` §4) | rewritten to the code in the slice that changes them | the contract's own rule | 2 to 4 |
| Step 3b design's C7 | pending | withdrawn: nothing adopts | — | 2 |

---

## 17. Questions for the owner

| # | Question | This design's answer |
|---|---|---|
| Q1 | Is SiteId a point or a counter (ID-13)? | A point. A counter is session history and breaks step 6's two-process hash gate. |
| Q2 | Is a `poly` kept or replaced whole? | Whole. An edit inside an instrument's voice graph restarts that instrument, as today; everything else keeps. A rewired instrument is kept and its notes released. |
| Q3 | Where do structural fades fall (RENDER-25a)? | On changed edges into kept nodes and on the output, linear and exact when equal; replaced nodes ring out for 5 ms as ghosts. |
| Q4 | What happens on a reorder of kept effects? | The reordered effects restart (the cycle rule). Nothing else does. |
| Q5 | What does "pause" mean during an export with plugins? | A tape pause: a 5 ms fade out, frozen, then a 5 ms fade back in where it was; kernel tails resume and plugin voices were reset. Transport pressed meanwhile applies at the resume. The alternative is a stop: every note ended, and play pressed again. |
| Q6 | `LoadState` and `SetArrangement` as engine messages? | Neither: a main-thread call, and an activation of the same structure. |
| Q7 | RENDER-1's call order? | `save_state` first; `reset` after `activate`, on the render thread. |
| Q8 | May activations coalesce (RENDER-28)? | Never; each names its plan. Step 3b's C7 is withdrawn. |
| Q9 | A plain number (not a declared control) edited inside an instrument body? | It is part of the voice shape, so it restarts the instrument. Declared controls are cells and don't. |
| Q10 | Committed values through activations (§9.3)? | Yes, once measured: one path, and smooth controls ramp. The cost is a few ms more than a direct cell write; a lease's own value is still heard at once. |
| Q11 | A plan that went NaN or trapped? | Its kernels restart at the next activation (plugins and lanes keep), or in place on `Panic`. That is today's behaviour for that case. An automatic recovery without an edit could come later. |
| Q12 | std's mixer under ID-14 | Adding a channel resets every strip inside the mixer's `map`. That is inaudible today (`std:gain`'s smoother starts at its target; pan holds no state), but a stateful effect inside such a map would lose its tail. Proposed std follow-up, outside step 7: key each strip by its channel key. |

---

## 18. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | One Store per instance at about 6,000 costs too much: about 25 TiB of 4 GiB reservations near the address-space ceiling, slow `Instance::new`, or scattered-store call overhead | G7f measures first, with a written rule; P3 and P2 fall back in the same code shape; the arena removes today's 2× peak at every activation |
| R2 | The ghost set grows to the whole song (a package change, a master rewire with everything replaced) | 5 ms at 2× DSP, which is today's cost on every edit, now only on rare ones; G7f part 2 includes one |
| R3 | During the window, a ghost reading a kept node gets that node's new output, not its old one | A kept instance has one output. It is still click-free (G6), just not a perfect A/B for 5 ms. |
| R4 | A blanked voice compare misses a structural difference, so a pool moves into a graph it doesn't fit | `Voice` `PartialEq` on everything but values, origins, names and capture targets; the audio thread's shape hash; a debug node-count check at entry; `a_changed_shape_starts_from_init` covers voice changes |
| R5 | Myers misaligns repeated tokens (many identical `kernel(` calls, one deleted) and hands a neighbour's state over | Bounded by shape and ID-14 counts; can't click, since consumers' changed inputs fade; ID-14 accepts the same exposure for reorders |
| R6 | The export's hold races something: a `LoadState` on a borrowed instance, or an activation | While `exporting`, the actor sends no `Activate` and makes no host call on a live instance; `Activate` can't apply while held anyway; the guard always restores and resumes; probe CI test |
| R7 | Slice 3 changes delta spellings, so an older `Seen(hash)` stops matching | A stale hash is a `Conflict` that rechecks, never a wrong write; noted in slice 3's commit |
| R8 | "A rebuilt kernel module" is reachable only in a runtime test, since packages load once per session | The shape compares content hashes, so any future reload path is safe by construction; G7d tests the rule directly |
| R9 | A plugin glitches on `load_state` while active, or under deactivate and activate | The same calls every host makes for presets and bounces; the "loading" status shows; HOST-3 later isolates crashes; only Surge XT and Airwindows are gated |
| R10 | The slices collide with the data-plane workflow in `crates/server`, `crates/gates`, `core/session` and `web` | Slice 1 is runtime and play only; the rest rebase after it lands; `SwapLane`'s address is kept so its lane previews don't change |
| R11 | Until slice 3, the edited track still restarts its wrapped instrument | That is today's behaviour; slice 2 already meets the gate's words (untouched tracks); slice 3 follows directly |
| R12 | The cycle rule is new code with no precedent here | `an_effect_reorder_replaces_the_cycle_and_keeps_the_rest`; random structural edits in `live.rs` |
| R13 | `munmap` storms on the dropper thread stall the audio thread through the VM map lock | Measured in G7f against today's whole-store drop on every edit; batching only if worse |
| R14 | The value fast path's deletion collides with the data-plane workflow's faster Tx path | §9.3 makes the deletion conditional; nothing else depends on it |
| R15 | The activation block's own cost (cell compares, wrappers' first copies) | G7b bounds it at 1.25× the median callback; only differing cells are written |
