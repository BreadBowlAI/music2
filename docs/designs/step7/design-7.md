# Step 7 design: the engine

Against `docs/architecture.md` (normative) at HEAD `cbf5eb9`. **Revision 2**: the merged design with
the attack folded in. §17 lists each problem raised, the verdict, and where the text changed;
revision 1 is kept beside this file as `design-7.rev1.md`. The base is `design-simple.md`, with the
parts of `design-robust.md` that close gaps a producer would hear. Both build on the three survey
notes in this folder (`understand-runtime.md`, `understand-identity.md`, `understand-contract.md`).
Nothing was built or run, and no cargo command was used. Files the data-plane workflow is editing
were read as HEAD has them (`git show HEAD:…`). Every cost marked *estimate* is unmeasured; G7f and
the slice measurements replace it.

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

### 0.1 The two input designs against the four criteria

| Criterion | design-simple | design-robust |
|---|---|---|
| **Meets every gate** | Yes, with one defect: reordering two kept effects makes the 5 ms window's graph cyclic, and its prepare has no rule for that. Its G7b taps every untouched track and demands bit identity. Its fade is exact when old and new are equal. | Yes. It has a cycle rule. Its G7b checks only the whole mix, and allows 1e-6 of difference inside the window. |
| **Simplicity** | Fewer concepts: four engine messages, `Renderer` keeps its name and API. But an audio-node box, a free-running ghost audio node and `PlanInfo.audio`. | More concepts: `PlanId`, `Next`, `Data`, `PlanIndex`, poison bits, `Hold`, a union order, a cycle rule, batched drops. |
| **A producer never hears a regression** | Gaps: a smooth control steps at activation; a NaN'd or trapped node keeps its state across edits; a ghost reading a removed control lane steps. | Closes the ghost-lane gap and trap persistence; replaces a `poly` whose lane changed, cutting its release tails. |
| **Buildable in green slices** | Yes; slice 1 touches only `core/runtime` and `core/play`. | Yes, but slice 1 starts in files the data-plane workflow is editing. |

### 0.2 Decision

The base is **design-simple**: the fewest concepts, a slice order that fits the data-plane workflow,
exact fades, per-track tap gates, and keep-and-release for a rewired instrument.

**Grafted from design-robust:**

1. **The cycle rule** (§6.6).
2. **Verify, then move** (§7.2). The audio thread checks the base plan and a 64-bit shape hash for
   every move and refuses the whole activation if any check fails; the actor recovers through the
   existing `Way::Replace`.
3. **Lanes are engine state** (§8). Schedulers, control lanes and audio players persist by
   `LaneAddr`. A lane only a ghost still reads stays as a ghost lane for the window. Audio-lane and
   asset edits ride in an `Activate`, which is the path RENDER-27 already names; `SwapLane` stays
   exactly as HEAD has it, for notes and control.
4. **Cells carry across an activation** (§9): a continued cell whose value changed is applied as a
   `SetCell` would be, so a smooth control ramps in a mixed edit. The value fast path stays.
5. **A plugin's last-sent parameter values continue with it** (§10.1), made safe by sending a
   constant whenever it differs from the last value sent.
6. **`Hold` for export** (§10.5).
7. **A plugin restart is prepared at once**, from the committed blob (§10.3).
8. **Leak counters and a churn test** (§12.7).
9. **G7f's extra metrics** (§11), measured on the runtime itself.

**New in the merge:** a box whose output went non-finite in its last block, or that trapped, is reset
as it moves into the next plan (§7.7). Nothing else is touched, and nothing is shared with prepare.

**Dropped:**

- from robust: the `Plan`/`Data`/`Next`/`PlanIndex` split and an engine-owned transport; poison bits
  shared with prepare; batched drops built unconditionally; a poly shape that includes its lane;
  identity fixes before the arena; deleting the value fast path;
- from simple: `Shape::Audio`, the ghost audio node and the lend;
- from revision 1 (the attack): the plan-wide taint flag and `Basis::Keep.wipe`; audio `SwapLane`
  with its own readiness rule; equal-power audio swaps through re-pointed taps; F7's second pass for
  keyed plugins; the export's save-and-restore; restart's save round trip; `continues` on every
  settle; §9.3's conditional deletion; G7f's wasmtime re-implementation and its T layout.

---

## 1. The design on one page

**One rule.** Every stateful node of a plan holds its instance in a box. An activation moves boxes
from the plan that plays into the new plan, at a block line, by a move list computed off the audio
thread. Every move is a `mem::swap`, so whatever the move displaces leaves inside the old plan.
Whatever isn't moved leaves with the old plan and is dropped off the audio thread. The boxes are:

- a kernel instance, which owns its own wasmtime `Store`;
- a whole `poly` voice pool;
- a plugin instance;
- a feedback ring.

Lanes (notes schedulers, control lanes, audio players) move the same way, by `LaneAddr`. "The arena"
is just the boxes in the playing plan's slots. There is no `RuntimeId` type and no map.

**Identity.** A plugin with a state path is identified by its module and that path (ID-8), exactly as
HEAD's `CarryKey` does, from slice 2. Every other node is identified by step 6's `GraphDelta` names,
compared between what plays and what is accepted: `m2_session::tx::continues` joins those names to
flat node indices. The runtime then keeps a node only when its shape matches: the kernel id and its
package's wasm SHA-256, the channels, a poly's blanked voice structure, a plugin's id and version.
Slice 3 fixes four naming defects, so wrapping an instrument in an effect keeps the instrument.

**Fades** go only on changed edges into kept nodes, on the song output, and on an audio lane whose
renders change. All are linear over 5 ms through one function, and bit-exact where old and new are
equal. A replaced or removed node runs on as a *ghost* for that window, with its notes ended, then is
retired. No instance is ever processed twice. An edit that would make the window's graph cyclic (a
reorder of kept effects) replaces the nodes on the cycle.

**Engine messages:**

| Message | Carries | Status |
|---|---|---|
| `Activate` | a prepared `Renderer` with its `Entry` | as today |
| `SwapLane` | notes and control lane data | as today, unchanged |
| `Transport` | a command | as today |
| `Panic` | — | as today |
| `Hold(bool)` | pause or resume, for export | new (slice 4) |

These become `Activate`s of the same structure, keeping every instance they can: a tempo or meter
change, an audio-lane or asset edit, a plugin state change, a notes lane that outgrows its scheduler,
and a plugin restart. A committed value alone stays a cell write, as today. There is no
`SetArrangement` and no `LoadState` message: a state load is a main-thread call made while the plan is
prepared.

**Export** of a song with plugins:

1. `Hold(true)` fades the engine out and stops it.
2. The export thread puts each live instance, shared by `Arc`, in offline mode holding the committed
   blob, and renders with fresh kernels (RENDER-1).
3. It puts them back in realtime mode, and `Hold(false)` fades the engine back in where it was.
   Nothing is saved or restored: the instance already holds the committed blob.

**Store:** one per kernel instance. G7f measures HEAD's `Renderer::new` against slice 1's on p50,
with a rule written in advance. The fallbacks are the pooling allocator or a smaller memory
reservation, never a shared store.

**Build order**, four slices, each green (§18):

1. Movable instances, no audible change (can start now).
2. The arena.
3. Names that survive edits.
4. Plugins persist: state loads into kept instances, export.

---

## 2. What a producer must not notice, and what keeps it so

| # | Heard | How it would happen | What rules it out | Checked by |
|---|---|---|---|---|
| P1 | A tail cut on a track nobody touched | a rebuild of every instance (today); names that shift | instances move and are never rebuilt (§7.2); an untouched node keeps its inputs, so no fade touches it (§6.3) | G7b, bit for bit per track |
| P2 | A click at the edit | a one-sample switch; a fresh node's first sample; a kept node whose source changes; an audio lane's renders replaced | every changed edge into a kept node, the output, and a swapped audio lane fade over 5 ms; a fresh node reaches a kept node only through a changed edge (§6.3, §8.2) | G6 `step`; `an_inserted_effect_fades_dry_to_wet`; `an_audio_clip_edit_crossfades_and_keeps_the_rest` |
| P3 | A level swell at an audio edit | an equal-power crossfade of the same material sums to +3 dB | audio swaps crossfade linearly, exactly where the two sides are equal; equal-power only at jumps, whose sides are different material (§8.2) | `an_audio_swap_of_unchanged_clips_changes_no_bit`; `a_clip_gain_edit_stays_between_its_two_levels` |
| P4 | A smooth control jumping, or buzzing | a continued cell whose value changed, written as a step at activation; a voice-cell ramp in flight moved into the new plan and copied every block | cell carry applies it as a `SetCell` from the old value (RENDER-22a); the old plan's ramps are ended before anything moves (§9.1) | `a_committed_smooth_value_ramps_through_an_activation`; `a_kept_polys_voice_cells_take_the_new_values` |
| P5 | A stuck note | a kept instrument rewired to another lane; a scheduler replaced under a kept consumer; note ids colliding in a plugin; a stop, seek or wrap on the activation's block line hidden by `Jump::Enter` | schedulers persist by `LaneAddr`; a rewired consumer is released at entry; note ids are engine-wide; a real jump wins over `Enter` (§8.1) | G4 `stuck`; `a_jump_on_the_activation_line_ends_its_notes`; `random_edits_and_jumps_never_strand_a_gate` with jumps on activation lines |
| P6 | A plugin losing its patch, or an edit not reaching it | a new instance where an old one should continue; keyed plugins paired by position; a changed constant parameter never sent; a skipped load; code-owned parameters not re-sent | keyed plugins are identified by module and state path (§3.2); a constant is sent whenever it differs from the last value sent (§10.1); a state change loads into the kept instance, compared per node (§10.2); the host's skip check tracks what it *holds*; `reloaded` re-sends code-owned parameters | G7e; `swapping_two_plugin_keys_moves_the_instances`; `a_changed_constant_parameter_reaches_a_kept_plugin` |
| P7 | An export different from what was committed | the live engine processing an instance during the export; no reset | `Hold` proves no plugin is called before any host call; reset on the render thread after activate; kernels fresh (§10.5) | G7e probe: borrowed export equals a fresh one bit for bit |
| P8 | Stale kernel memory after a rebuilt package | matching by kernel id only | the shape includes the package's wasm SHA-256, checked at prepare and again on the audio thread (§5.2, §7.2) | G7d, with an identical-bytes control |
| P9 | A track silent after a NaN or a trap, across every edit | a NaN'd ring, a filter whose state went NaN behind a clipper, or a trapped kernel, kept across edits | a box whose output was non-finite in its last block, or that trapped, is reset as it moves; a single stray sample clears at the next finite block (§7.7) | `a_box_that_went_non_finite_is_reset_when_it_moves`; `a_transient_inf_keeps_every_instance`; `a_trapped_kernel_is_reset_at_the_next_activation` |
| P10 | A dropout at the edit | today's 2× DSP for 2 blocks; allocation or deallocation on the audio thread | one plan; only ghosts run twice; every list is sized at prepare; every move is a swap (§7) | `no_alloc.rs` extended (it counts deallocations); G7b prints the activation block's time |
| P11 | Memory creeping up | a shared Store; retirements leaked; an old render kept by its player | one Store per instance; room to retire is checked before an activation applies; a swapped player's old renders retire at the window's end (§7.6, §8.2) | the churn test, with audio edits (§12.7) |
| P12 | Audio off the grid after a tempo change | unwarped clips keeping start samples from the old tempo map | every audio lane is rebuilt on a tempo change, warped or not (§8.4) | `a_tempo_change_keeps_every_kernel_and_the_beat`, with an unwarped clip after the change |
| P13 | Playback jumping back, or an edit never arriving | a copied transport going back; a dropped or coalesced activation | the transport is copied and retimed by beat; activations never coalesce; a refused one is recovered by `Way::Replace` (§7.5) | G5 `Sampler`; `an_activation_against_another_plan_is_refused` |

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
| **RuntimeId** | for a plugin with a state path: its module key and that path (ID-8). For every other kernel, plugin, `poly` or feedback node: its full delta name, InstancePath / `<op>@<file>:<point>` / `#<n>`. It never leaves the session. |
| **ControlId** | the name of a declared control's const, `@<slot>` (built). |
| **CellId** | an index in the active plan's cell table (unchanged). |
| **A voice's RuntimeId** | its slot in its kept pool (RENDER-16). |

### 3.2 `continues` (slice 2)

The one new session function, in `core/session/src/tx/continues.rs`:

```rust
pub struct Continues { pub top: Vec<Option<u32>> }

/// For each top-level node of the new revision's Flat, the node of the old revision's Flat it
/// continues. A plugin with a state path is identified by its module and that path (ID-8, ID-15);
/// every other node by its delta name (ID-13, ID-15) and, from slice 3, equal occurrence counts
/// (ID-14). Shapes are the runtime's job.
pub fn continues(old: &Revision, new: &Revision) -> Continues;
/// The same code: node i continues node i.
pub fn identity(flat: &Flat) -> Continues;
```

How it works:

1. **Path pass.** Each new `FOp::Plugin { state: Some(p), .. }` takes the old `FOp::Plugin` of the
   same module (the `<module>/` prefix of `FNode.name`) whose `state == Some(p)`.
   `plugin.state-collision` (`core/ir/src/check.rs`) makes `(module, path)` unique on each side, so
   this is a hash join of about 15 lines. Both nodes are claimed.
2. **Name pass,** for every node left unclaimed on both sides:
   - build the same `Coords::revisions(Edits::between(..))` per changed code file as `delta_between`,
     and call `delta::compare`, which already returns both sides' `Graph`s;
   - expose the `ids: HashMap<String, Name>` that `Side::scope` builds per module (about 10 lines),
     so each `Graph` maps (module key, IR node id) to its name;
   - for each unclaimed top-level `FNode` of the new `Flat`: split `FNode.name`
     (`<module>/<node id>`), look up its name, look for the same name among the old side's unclaimed
     nodes, and find the old `FNode` by its `<module>/<id>`. Pure nodes (consts, lanes, time,
     `Missing`) are joined too: the runtime uses them to decide whether an input's source is "the same
     source" (§6.3).
3. Nodes inside voices aren't joined: a `poly` moves whole (§5.1).

So a keyed plugin follows its key whatever its place: two keys swapped, an array of keys reordered, a
keyed Surge wrapped in an effect. That is HEAD's guarantee (`CarryKey` includes the state path;
`keyed_instances_follow_their_keys` in `core/runtime/src/tests/live/carry.rs`), kept in every slice.
A keyed plugin whose path the old side doesn't have (a new or renamed key) goes on to the name pass,
so adding a key to a plugin keeps its instance once slice 3's names survive that edit.

Where it is called:

- **Only when the actor prepares an `Activate` for a revision whose code changed.** `reach`
  (`crates/server/src/graph/actor.rs`) keeps calling `delta_between` on every settle, unchanged, and a
  `None` from it still means "activate". Only that branch calls `continues`, so a note edit pays
  nothing for it and the data-plane workflow's path is untouched.
- **It recomputes the comparison** for the changed code files and joins: about 1 to 3 ms
  (*estimate*), on code edits only, printed by G7a. If that grows, `delta::compare` can hand its
  `Graph`s to the join instead.
- **When the code is the same** (`code_rev` unchanged: a tempo change, an audio or state edit, a lane
  that outgrew its room, a restart, a recovery), the actor uses `identity` and names nothing.

Why slice 2's names are enough for the gate:

- Untouched tracks are in unchanged files, so their names are `Base` points on both sides. Package
  (std) sites are always `Base`.
- So every node of an untouched track, and the mixer in an untouched `song.ts`, continues.
- The edited file's calls whose first or last byte falls in a line hunk are renamed. For an unkeyed
  instrument wrapped in an effect, that includes the instrument (identity notes F2), so in slice 2 that
  one track restarts its instrument, as it does today. A keyed plugin is kept by its path. Every other
  track keeps everything.

### 3.3 Names that survive edits (slice 3)

The four defects from `understand-identity.md` §3, plus occurrence counts. All are on the engine's path
(`delta_between`), so every writer gets them, agents included (PLANE-1).

| Fix | What | Where | Size |
|---|---|---|---|
| **F1** | refine each line hunk with a token-level Myers diff: identifiers and numbers, strings, single punctuation characters, with whitespace and comments in the gaps; reuse the generic `kept_lines`; past `DIFF_LINES` tokens keep the hunk whole | `core/session/src/text.rs` `Edits::between` | about 80 lines |
| **F2** | name a call by the point of its `(`: a new `Site.open: u32`, found by scanning from the callee span's end (or the type arguments') past whitespace, comments and `?.`. `Facts.spans` keeps the whole range, so judging `Confined` is unchanged | `core/script/src/sites.rs` `visit_call_expression`; `delta.rs` `Side::place`, `Side::spell` | about 35 lines |
| **F3** | put the ComponentId in each chain segment, so a component swapped at one `(` is new and an alias (`delay as echo`) is not (PKG-9) | `delta.rs` `Side::chain` | about 5 lines |
| **F6** | `key()` pins: drop every segment above the innermost `key:` segment, since keys are unique per parent key frame (`keyFrames` in `core/script/sdk/core.ts`) | `delta.rs` `Side::chain` | about 10 lines |
| **F5** (ID-14) | `Facts.counts: Vec<u32>`: for each chain segment with an occurrence, how many frames share its (parent, site), then the size of the node's own `#n` group; the name pass keeps a pair only when the counts are equal | `delta.rs` `chain`, `name_all`; `continues.rs` | about 25 lines |

Revision 1's F7 (a second pass for keyed plugins) is gone: the path pass of §3.2 does it first, in
slice 2.

- **The delta's hash changes its spelling once.** It stays deterministic, so step 6's two-process gate
  holds once its expected names are updated.
- **Optional, off the critical path:** `crates/editor/src/controls.rs`'s `carry_text`, `carry_ends`
  and `carry_lines` become `text::Edits::between(old, new).carry(range)`, one correspondence for
  leases, cables and undo (WRITE-12, WRITE-22). `m2_ir::control_id`, which only a test uses, is
  deleted.

### 3.4 Properties of the match

- **Injective.** The path pass is unique by `plugin.state-collision`; names are unique (the
  `debug_assert` in `scope`, and `delta_names_are_unique_on_every_fixture`); the name pass takes only
  unclaimed nodes on both sides. `prepare` checks again, and a `from` that repeats is a bug assertion.
- **Never across modules.** Both passes stay inside a module key. A step 8b key rename can add a pass
  over `ModuleChange::Rekeyed`.
- **Shape-gated.** A wrong match can only pair same-kind, same-shape instances (§5.2).
- **Ambiguity resets.** A changed count resets the whole group (slice 3). A node with no partner is
  fresh.
- **Which way errors lean.** A false *keep* hands a same-shape neighbour's state over, and the
  consumers' changed inputs still fade, so it can't click. For a plugin, a pairing across different
  paths loads the right blob from slice 4 (§10.2); before it, the shape's state hash refuses a pair
  whose blobs differ. A false *reset* cuts a tail, which is what G7b looks for.
  After slice 3, the remaining false resets are a reformat of a whole track call (only that track) and
  a cut-and-paste (deferred, with ID-13's rule 4).

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
    fade: u32,                     // round(0.005 × rate) frames when anything fades or an audio lane swaps; else 0
    faded_from: Option<u64>,       // the engine clock at entry, set by take_over
    leftovers: Leftovers,          // room for every ghost exec, ghost lane and swapped player's old renders
    info: Arc<PlanInfo>,           // never read on the audio thread
}

pub(crate) struct Lanes {
    slots: Vec<LaneSlot>,          // live lanes, then ghost lanes (§8.2)
    refs: Vec<LaneRef>,            // module, lane, kind: what `lane_slots()` gives
    live: usize,                   // slots[live..] are read only by ghosts
}

/// Every variant can be empty, so a lane move is a `mem::swap` (§7.2).
pub(crate) enum LaneSlot {
    Notes(Option<NoteScheduler>),
    Control(Option<Arc<ControlLane>>),
    Audio(Option<Box<AudioPlayer>>),   // today's AudioNode state, plus its renders (§8.2)
}
```

What is built from what:

- **`Plan`** from the `Flat` and the packages only: `Builder::graph`, the exec wrappers, fresh boxes,
  `cells(flat)`, and the ghost and fade nodes.
- **`Lanes`** from the `DataSet` and the tempo map: `NotesData::build`, `data.control()`, and
  `AudioLane::build` through `audio::Cache`.
- **A change of notes or control data alone** never builds a `Plan`: `SwapLane` replaces lane data in
  the running `Lanes`, as today.
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
        changed: &'a LaneChanges,         // lanes the delta lists, and whether the tempo map changed
        restarts: &'a HashSet<Handle>,    // §10.3: never kept; new from the committed blob
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
| `Plugin(PluginNode)` | `inst: Option<Arc<PluginInstance>>` (with `shape: u64`) | `audio_in`, `notes`, `params: Vec<(u32, Input, Option<f32>)>`, `keys`, `ended`, `release`, event lists (sized by caps), `status`, `ghost: bool` | `Plugin { id, version, channels, state: Option<Sha> }`; `state` goes in slice 4 |
| `Feedback { input, ring }` | `ring: Box<[f32]>`, swapped | `input: PortRef` | `Feedback { channels }` |

- **Every stateful wrapper also has `poisoned: bool`** (§7.7). `take_from` copies it with the box.
- **Nothing else holds state that outlives a plan.** That covers `Const`, `Time`, `Control`, `Notes`,
  `Missing`, `NoteField`, `Capture`, the new `Fade`, and `Exec::Audio { lane }`, which copies its
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
    instrument (owner question Q5). Declared controls are cells and don't.
  - The `Arc<Voice>` is the one in the `Flat`, so the blanked copy is made once per poly at prepare.
- **A plugin's state path is not in its shape.** The path pass makes it equal for every pair it makes,
  and a pair across paths loads the right blob (§10.2). Putting "has a blob" in the shape would make
  an instance's own first save (no blob → a blob) restart it.
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
    pub plugins: Vec<PluginAt>,                // { node, inst: Arc<PluginInstance>, path: Option<Vec<String>>, blob: Option<Sha> }
    pub index: HashMap<String, usize>,         // FNode.name -> node: gates' taps, export's borrow
    pub cell_slots: Arc<CellSlots>,
    pub cell_map: Arc<CellMap>,
}
```

- **Where it comes from:** `prepare` writes it, and the server keeps it as `Plays.info`. It replaces
  `Plays.{slots, caps, cell_slots, cells}` and `Open.carry`.
- **It is immutable, and describes the plan as it will be once its activation applies.** The next
  prepare reads only `PlanInfo` and the old `Flat` (`Plays.revision`), never the live plan, which
  belongs to the audio thread.
- **`PluginAt.blob`** is the committed blob's hash the instance was created from or last loaded with
  (slice 4 compares it, §10.2).
- **Why `Plays.revision`'s `Flat` is the right old `Flat`:** `adopt` moves it forward only when the
  graphs are equal but for constant values (`value_writes`), so its node indices are the playing plan's.
- **Plugin lifetimes:** the `Arc<PluginInstance>` clones keep an instance alive until the `PlanInfo`
  that names it is replaced. That happens at the next settle on the actor thread, where its destruction
  is queued to the plugin main thread, as today.

---

## 6. Prepare, off the audio thread

Inputs: the old `Flat` O, its `PlanInfo` I, the new `Flat` N, `cont`, `changed`, `restarts`, and the
new `DataSet`. `prepare` never touches a live box. A prepare that fails (for example, a plugin that
won't load) changes nothing that plays: its fresh instances drop on the actor thread, and the old plan
plays on. The only live call it makes is slice 4's state load into a kept instance, made last, after
everything that can fail (§10.2).

### 6.1 Lanes

For each lane that `lane_slots(N)` lists, by `LaneAddr` against `I.lanes`:

- **In I with the same kind:** a `LaneMove { from, to, swap, grow }`.
  - `swap` is new data when the delta lists the lane, or when the tempo map changed: then every notes
    lane gets `NotesData` built in the new map, and every audio lane gets renders through the cache,
    warped or not (§8.4). It is an `Arc<NotesData>`, an `Arc<ControlLane>`, or an `Arc<AudioLane>`.
    Otherwise it is `None`.
  - `grow` holds a fresh `NoteScheduler` sized for the new data, when its `needs` exceed the old caps.
  - An audio `swap` sets the plan's `fade` to 5 ms, so a window runs and retires the old renders at its
    end (§8.2).
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
- (slice 4, plugins) not "the old node had a committed blob and the new node has none": CLAP has no
  "load defaults".

A kept node claims `j`, and `Move { from: j, to: i, shape: hash64 }` goes on the move list.

Every other stateful node gets a fresh box:

- **Kernels:** `Store::new`, `Instance::new`, typed funcs, `k_init`, the port checks and the prewarm
  reset, as today, with one store each.
- **Plugins:** `host.create` from the committed blob, then `activate`, on the plugin main thread while
  the old plan plays.
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
- a plugin's `in` (parameters aren't faded: a changed parameter is a parameter event, which plugins
  smooth themselves, and both parameter arms send whenever the value differs from the last one sent,
  §10.1);
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

### 6.5 Fade nodes

For each changed edge, append `Exec::Fade { old: Input, new: Input, channels: u8 }`:

- `old` is the remapped old input, or `Const(0.0)` when it was `Unset`;
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
- **A kept instance whose committed blob changed** gets `load_state` (slice 4, §10.2), as the last step
  of prepare, after every fresh instance exists.

### 6.9 The entry, all sized here

```rust
pub(crate) struct Entry {
    after: PlanId,                     // the plan it was prepared against
    moves: Vec<Move>,                  // Move { from: u32, to: u32, shape: u64 }: kept nodes, then ghosts
    lanes: Vec<LaneMove>,              // { from, to, swap: Option<LaneSwap>, grow: Option<NoteScheduler> }
    cells: Vec<(CellId, CellId)>,      // §6.7
    voice_cells: Vec<CellId>,          // §6.7
    release: Vec<u32>,                 // new exec indices whose notes end at entry: ghosts, rewired (§3.5)
    pub stats: Stats,                  // kept, fresh, ghost, fade and reset counts by kind (gates)
}
```

`LaneSwap` gains an `Audio(Arc<AudioLane>)` variant that only a `LaneMove` carries; the `SwapLane`
message refuses it, as it refuses any kind that doesn't fit its slot today.

`Leftovers` (on `Plan`) has room for every ghost exec, every ghost lane, and the old renders of every
audio lane the entry swaps. It is where they go at the window's end.

### 6.10 Worked examples (p50, track `t02`, pluck into the mixer's gain)

| Edit | Kept | Fresh | Ghosts | Fades |
|---|---|---|---|---|
| wrap pluck in `delay(…)` (slice 3 names) | pluck's `poly`, gain, everything else | delay | none | `gain.in`: pluck → delay |
| the same, slice 2 names | everything but t02's pluck and delay | pluck, delay | the old pluck (released) | `gain.in`: old pluck → delay |
| remove that delay | pluck, gain | none | delay, reading pluck's output | `gain.in`: delay → pluck |
| `lowpass(` → `highpass(` at one call | everything but the filter | highpass | lowpass | the filter's consumer's input |
| reorder `comp(eq(x))` → `eq(comp(x))` | everything but comp and eq | comp and eq (demoted by the cycle rule) | the old comp and eq | the consumer's input |
| a tempo change | everything | none | none | none; every notes lane gets new data and every audio lane crossfades to its new renders (§8.4) |
| a clip's gain on one stem lane (`examples/resample`) | everything | none | none | none; that lane's player crossfades, its other clips bit-exact (§8.2) |
| a rebuilt `std.wasm` (G7d) | plugins and lanes only | every kernel and voice pool | the whole old kernel graph | the song's output |

### 6.11 What an activation costs at p50 (*estimates*)

| Part | Today | After |
|---|---|---|
| Kernel instances built | about 6,100 | the new ones: 1 to 3 for an added effect, about 156 for a replaced pluck |
| Plan-side wrappers and buffers | inside the build | about 2,500 top-level nodes, about 2.5 MB zeroed: 1 to 3 ms |
| `continues` | the delta alone | on code edits only: the comparison again plus the join, 1 to 3 ms |
| Audio-thread entry | `enter_from`, `carry_from` | about 300 moves, about 100 lane moves, about 1,500 cell compares: tens of µs |
| CPU during the 5 ms | twice the whole song | the song plus the ghosts, plus a second fill per swapped audio lane |
| Audio thread, every block | — | the non-finite check of about 300 stateful outputs (about 77,000 samples): under 20 µs |
| Teardown on `m2-play-drop` | about 6,100 instances unmapped | the replaced ones, plus plan buffers |

---

## 7. Activation, on the audio thread

### 7.1 Messages and readiness

```rust
pub enum Msg {
    Activate { renderer: Box<Renderer> },                         // carries its Entry
    SwapLane { generation: u64, slot: u32, data: LaneSwap },      // as today: notes and control
    Transport(Cmd),
    Panic,
    Hold(bool),                                                   // §10.5, slice 4
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

- no fade window is running (a plan that swaps an audio lane always has one, so this also covers a
  player's crossfade);
- the engine isn't held or holding;
- the stash has room for 3 retirements: the old renderer now, the leftovers at the window's end, and
  one to spare.

Otherwise it waits at the head of the queue, as today, for at most 2 blocks at 48 kHz. **`SwapLane`**
is ready as at HEAD. **`Hold(true)`** is ready when no window is running, and **`Hold(false)`** is
always ready.

### 7.2 `Renderer::take_over(&mut self, old: &mut Renderer) -> bool`

This runs in order, with no allocation, no deallocation, no lock and no drop. It verifies everything
first. Every move is a `mem::swap` (or `Option::take` into an empty slot), so whatever a move
displaces stays inside `old`, or inside the spent `Entry`, and leaves the thread with it.

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
   - the new slot is empty;
   - a notes `swap` fits the moved scheduler's caps, or a `grow` is given;
   - an audio `swap`'s player has no old renders still fading (it can't, by §7.1; a bug assertion).

**Then move:**

0. **End the old plan's ramps** as its next block line would have: `old.plan.cells.end_ramps(&mut
   old.plan.graph)`, the first loop of `Cells::apply` (`Write::Step(value)` for each cell on
   `ramping`). A voice-cell ramp written in the last block would otherwise move with its pool and be
   copied every block (§9.1).
1. **Boxes.** For each `Move`, `self.exec[to].take_from(&mut old.exec[from])`:
   - *Kernel:* `mem::swap(&mut self.inst, &mut old.inst)`.
   - *Poly:* `mem::swap(&mut self.pool, &mut old.pool)`.
   - *Plugin:*
     - `inst` is swapped;
     - `keys`, `ended` and `status` are copied within their room;
     - the last-sent values in `params` are copied by parameter id, a merge walk over two lists sorted
       by id;
     - for a node flagged `ghost`, or on `release`, the keys go to `release` instead, to get a NoteOff
       at its first call.
   - *Feedback:* `mem::swap` of the rings. The prepared ring goes back with the old plan.
   - *Every kind:* `poisoned` is copied. When it is set, the box is reset in place, as `Panic` resets
     it (kernel `k_reset`; a pool's voice graphs reset and their notes dropped; a plugin's `host.reset` with its keys moved
     to `ended`; a ring zeroed), and the flag is cleared (§7.7).
2. **Song position.**
   - `self.transport = old.transport`, then `retime(old.tempo → self.tempo)` and `enter()`, as
     `enter_from` does today.
   - `self.clock = old.clock` and `self.next_note = old.next_note`.
   - A Start, Stop, Seek or wrap pending on this block line is carried in the copied transport and wins
     over `Enter` (§8.1), so moved schedulers end their notes and moved players declick as on any
     block. `Enter` alone re-seeks each scheduler's cursor without ending notes, which changes nothing
     for a moved scheduler.
3. **Lanes.** For each `LaneMove`:
   - **Plain:** `mem::swap(&mut self.lanes.slots[to], &mut old.lanes.slots[from])`. The new slot was
     empty, so the empty one goes back to `old`.
   - **With a notes or control `swap`:** the data is taken from the `LaneMove` into the moved entry,
     and the replaced data is put back into the `LaneMove`, so it leaves with the spent `Entry`.
   - **With an audio `swap`:** `player.swap(new, shift)`, with `shift = old.transport.pos() as i64 −
     self.transport.pos() as i64`, which is nonzero only when step 2 retimed. The player's current
     renders become its `prev` and fade out over the window (§8.2). The `LaneMove` keeps nothing.
   - **With `grow`:** the prepared scheduler `take_state(&mut moved)`. That copies `sounding` (it fits,
     because it is bigger) and sets `swapped`. The prepared one is swapped into the slot, and the small
     one goes back into `old.lanes.slots[from]`.
4. **Cells.** Cell carry (§9.1).
5. **Release.** For each node on `release`:
   - a poly: every voice with a note gets `note.off = min(off, clock)`;
   - a plugin: done in step 1.
6. **Window.** Set `faded_from = Some(clock)` when the plan's `fade` is nonzero.
7. **Retire the entry.** `mem::swap(&mut self.plan.entry, &mut old.plan.entry)`. The spent entry,
   holding the replaced lane data, leaves inside `old`.

`Engine::try_apply(Activate)`:

- **Refused:** the new renderer is retired whole, unplayed. `refused_activations` is counted, the
  `Applied` stamp gets `refused: true`, and a `debug_assert` fires. The old plan plays on.
- **Applied:** `old` is retired whole (`Retired::Renderer`). It takes every box nothing moved, its plan
  buffers and the spent `Entry`. Then `current = renderer`.

**`AudioPlayer` and `Jump::Enter`.** The player's `Jump::Enter` arm becomes a no-op: it stops clearing
`fade_in`. A fresh player has none, and a moved one goes on with its own, so a fade-in in progress
isn't cut and a no-op activation stays bit-exact.

### 7.3 The window and its end

While the window runs, `Graph::process` follows the window's order: fade nodes mix, ghosts process,
ghost lanes play, and swapped audio players mix their two sides.

At each block line, before messages, once `clock ≥ faded_from + fade` and the stash has room,
`current.end_fade()` runs:

1. each ghost's `Exec` is swapped with `Exec::Const(0.0)` into `leftovers`;
2. each ghost lane is swapped with an empty slot of its kind into `leftovers`;
3. each swapped player's `prev` is taken into `leftovers`;
4. `mem::swap(&mut graph.order, &mut order_after)`, so the window's order stays in the plan until the
   plan retires;
5. `Retired::Nodes(leftovers)` leaves the thread.

Without stash room it waits a block: the ghosts play on, faded to nothing, and each finished player
reads only `cur`.

**Panic during the window** resets everything in the plan, ghosts included, and the window ends on
time. The engine never idles during a window.

### 7.4 Fades, exactly

One function, in `core/runtime/src/fade.rs`, used by `Exec::Fade` and by `AudioPlayer`'s swap
(§8.2). With `k` the engine frames since entry and `len = fade`:

```rust
#[inline]
pub(crate) fn xfade(o: f32, n: f32, k: u32, len: u32) -> f32 {
    if k + 1 >= len { n }                                   // after the window: an exact copy of the new side
    else if o.to_bits() == n.to_bits() { o }                // identical signals stay bit-identical, -0.0 included
    else { o + (n - o) * ((k + 1) as f32 / len as f32) }
}
```

- **Linear,** like today's whole-output fade, because the two sides are mostly the same music (dry
  against wet, the same instrument, the same clip). Equal-power stays only for an audio lane's jumps,
  whose two sides are different material (RENDER-9 item 5).
- **The equality branch** makes a silent structural edit, and an audio swap of unchanged clips,
  bit-exact through the window. That is what lets G7b and G7c demand bit identity.
- **After the window**, a fade node copies its new source only when that source's version changes,
  and takes its version. So a steady cell behind it still costs nothing per block (RENDER-22).

### 7.5 Message order (RENDER-28)

- **Activations are never coalesced.** Each move list is relative to the plan it was prepared against.
  The coalescing branch of `Engine::block` (`engine.rs:448`) is deleted.
- **The actor stays serial.** It prepares each activation against the plan it sent last. FIFO order
  makes that the plan playing when the engine reads it, so no acknowledgement is waited for.
- **`SwapLane` keeps its address `(generation, slot)`** and everything else HEAD gives it. The actor
  resolves the slot by `LaneAddr` in `PlanInfo.lanes` of the plan it sent last. A swap for another plan
  is refused and retired, as today. The data-plane workflow's lane previews (WRITE-22a) work as they
  are written.
- **Recovery from a refused activation.** The actor sees `refused` in the `Applied` stamps, and its
  next settle uses the existing `Way::Replace`: a fresh engine at the same beat, crossfaded by the
  player's `LiveFade`.
- **Recovery from a stall.** `Player::send` still reports `Stalled` after 500 ms and drops the message.
  The next settle uses `Way::Load` (a new player), as today. RENDER-28's text is updated to say so.

### 7.6 Retiring, and memory that can't grow

- **Every replaced object is retired off the thread.** It leaves in the old renderer, in the leftovers,
  or in the spent `Entry`. The room for each is checked before the activation applies.
- **`mem::forget` stays as the unreachable fallback,** and is now counted as `Engine.leaked`. The churn
  test requires 0.
- **One `Store` per instance** (§11) means that dropping a `KernelInst` frees its memory. No retired
  instance is pinned by a live one.
- **A swapped audio lane's old renders live one window,** then retire with the leftovers. `audio::Cache::prune`
  has already dropped its own reference, so the window's end frees them.
- **Counters.** `KernelInst::new` increments a static `LIVE_KERNELS`, and its `Drop` decrements it.
  With `PluginHost::live()` and the cache's renders, that is what the churn test compares.
- **Drops in batches.** The dropper (`drop_retired`, `core/play/src/lib.rs`) frees in batches only if
  G7f measures contention worse than today's drop of a whole store on every edit (§11.3).

### 7.7 Panic, idle, trap and non-finite output

- **Panic** resets the plan in place: kernels, plugins (`reset`), rings, voices, schedulers and audio
  players, and clears every `poisoned` flag. The next block is `Enter`.
- **Idling.** `can_idle` and `woken` read the plan that plays. The engine never idles during a window
  or a hold.
- **The flag.** Live, right after each stateful top-level node runs, `Graph::process` sets its
  wrapper's `poisoned` to whether its output holds a non-finite sample: one pass over channels × 128
  samples, about 300 nodes at p50, under 20 µs a block (*estimate*), no allocation. A kernel inside a
  voice is covered by its pool's output. A trap sets the flag on the node that trapped (its pool, for
  a trap inside a voice), and the plan goes `broken` (silent), as today. Offline renders stay strict
  (they bail at a non-finite output), so the check is live only.
- **What it does.** At the next activation, every moved box whose flag is set is reset in place (§7.2
  step 1), whether it is kept or a ghost. So a NaN'd delay ring, a filter whose state went NaN behind a
  clipper that turns NaN into a finite value, or a trapped kernel restarts at the next edit, and only
  that box does. The new plan is not broken. A single stray sample that a box recovered from clears
  with its next finite block and costs nothing.
- **Why this is enough.** Today any edit rebuilds everything and heals. The flag lives in the box and
  is read on the audio thread at entry, so prepare reads nothing, nothing races between prepare and
  apply, and no other tail is touched. A reset that itself traps leaves the new plan broken, which is
  what a trap in its first block would do; the edit after that tries again. No new trigger is added: a
  plan that broke stays silent until the next edit or `Panic`, as today.

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
- **A jump on the activation's block line.** `Transport::advance` (`core/runtime/src/transport.rs`)
  today gives `Enter` priority over every other jump. That was harmless while each activation brought
  fresh schedulers and released the old ones; with moved schedulers and players, an `Enter` that hides
  a Stop, a Seek or a block-line wrap would leave every sounding note held (`NoteScheduler::run` drains
  only when `jump != Enter`) and would cut a moved audio player without its declick. The order becomes:

  ```rust
  let jump = if !self.was_playing && self.playing { Jump::Start }
      else if self.was_playing && !self.playing { Jump::Stop }
      else if self.playing && self.seeked { Jump::Seek }
      else if self.playing && self.wrapped { Jump::Wrap }
      else if self.enter { Jump::Enter }
      else { Jump::None };
  ```

  For a moved scheduler or player the real jump does exactly what is needed. For a fresh one, Start,
  Stop, Seek and Wrap drain nothing and seek the cursor, which is all `Enter` did; a fresh player on a
  Seek or Wrap fades in over 5 ms, which only a kept consumer's fade or the `LiveFade` follows. It
  lands in slice 1, where it also fixes HEAD's adopted notes (C7) under a Stop on an entry line.

### 8.2 Audio players persist; their renders swap inside an activation

**Today.** The state (taps, fade-in, continuation) is in `Exec::Audio(Box<AudioNode>)`, one per graph
node. The renders are in `LaneSlot::Audio(AudioLane)`.

**After.** They join into one `AudioPlayer` per lane:

```rust
pub(crate) struct AudioPlayer {
    cur: Arc<AudioLane>,
    prev: Option<Arc<AudioLane>>,   // the renders an activation replaced, read for one window, then retired
    shift: i64,                     // prev's frame minus the transport's, fixed at the swap; 0 unless retimed
    since: u32,                     // frames since the swap
    decl: Declick,                  // AudioNode's state as today: taps, fade_in, len, prev_moving, cont, dropped (Copy)
    out: Box<[f32]>,                // channels × BLOCK: this block's output
    old: Box<[f32]>,                // channels × BLOCK: prev's side during a swap
}
```

- **One fill per lane.** The player fills `out` once per block before the graph, as schedulers run
  then. `Exec::Audio { lane }` copies it.
- **The same bits.** Two nodes reading one lane computed the same output from the same runs, so a
  single player gives the same bits (`one_player_per_lane_gives_the_old_bits`).
- **Jumps** (seek, wrap, stop, start) declick exactly as `AudioNode` does today: taps at the
  continuation, equal-power over 5 ms, because a jump's two sides are different material.
- **A swap** comes only in an `Activate`'s `LaneMove` (§7.2 step 3): `prev = Some(cur)`, `cur = new`,
  `since = 0`, `shift` from the entry. For the next `len` frames each block fills twice over the same
  runs:
  1. `prev` into `old`, with a copy of `decl` (on the stack, a few hundred bytes), reading at each
     frame plus `shift`;
  2. `cur` into `out`, with `decl` itself;
  3. each sample becomes `xfade(old, out, since + f, len)` (§7.4).

  So a clip the edit didn't change is bit-identical through the swap, and a changed one moves linearly
  between its two levels: no swell, because the two sides are the same music at the same position.
  Stopped, both sides are silent apart from a running stop tap, so there is no special case. A jump
  inside the window runs on both sides through the same `decl` logic.
- **On a retime** (§8.4) `shift` makes `prev` continue exactly what was playing, while `cur` starts at
  the same beat in the new map.
- **`prev` lives one window.** The plan's `fade` is 5 ms whenever it swaps an audio lane, and the
  window's end takes `prev` into the leftovers (§7.3). A replaced render never outlives the next block
  line with stash room.
- **Cache pruning.** `audio::Cache::prune` keeps every clip any lane of the new plan plays, moved
  players included, so an edit to one clip of a lane never re-renders its other clips.

### 8.3 What reaches the engine how (the actor's `way`, after slice 2)

`reach` is unchanged; only its `None` branch changes.

| The delta from what plays to the accepted revision | Path |
|---|---|
| empty | nothing |
| notes and control lanes only, each fitting its scheduler | `SwapLane`, as today |
| constant values only | `SetCell` through `value_writes`, as today |
| the code unchanged, and any of: audio lanes, assets, plugin states, the arrangement, a lane that outgrows its room, a plugin restart | `Activate`, prepared with `Basis::Keep` and `identity`; clip renders through the cache, and (slice 4) state loads |
| a code change, with anything else | `Activate`, prepared with `Basis::Keep` and `continues` |
| the rate changed; or the last activation was refused | `Way::Replace` |
| no player, or it stalled | `Way::Load` |

A revision that touches several classes takes the slowest row (PLANE-2). Clip renders happen on the
actor thread while the old data plays, as RENDER-27 asks; the 1 to 3 ms of plan building next to them
is noise.

### 8.4 Arrangement switches (RENDER-27)

A tempo or meter change is an `Activate` of the same structure:

- `identity` continuity, so every kernel, poly, plugin and ring is kept and there are no fades.
- Every notes lane gets a `LaneMove` with new `NotesData`, built in the new tempo map.
- When the tempo map changed, every audio lane gets a `LaneMove` with new renders, warped or not:
  `AudioLane::build` places every clip at `tempo.seconds_at(c.start) × rate`, and `render_clip` sizes
  it from `seconds_at(start + length)`. `ClipKey` includes the tempo map, so the cache renders what it
  doesn't hold, which on a tempo change is every clip (today's cost; R14). A meter change alone changes
  no `ClipKey` and swaps no audio lane.
- Control lanes are by beat, so they move unchanged.

At the block line, all of it switches together:

- the moves, the lane data, and the new `TempoMap` and `Arrangement`;
- the transport keeps its beat, recomputed into the new map;
- each moved scheduler with new data reconciles its sounding notes against it by `(clip, pitch, start
  beat)` at its first block (`sched.rs` `reconcile`). So each pending Off comes from the new data's end
  sample, which is RENDER-26a's "recompute the frame of every pending Off from its beat";
- each audio player crossfades from its old renders, continued at their own position, to its new ones
  (§8.2).

**No kernel can lose a tail on a tempo change.** A mixed change (code plus tempo, PLANE-2) is one
`Activate`, with fades only where the code changed.

### 8.5 Planes, as built after step 7

| Plane | Path |
|---|---|
| Data (notes, control) | `SwapLane`, as today |
| Prepared data: audio lanes, assets, plugin state | clip renders and (slice 4) state loads prepared off the audio thread, then an `Activate` of the same structure that keeps every instance |
| Arrangement | tables and clip renders prepared off the audio thread, then an `Activate` of the same structure |
| Code | compile, check, verify; `continues`; prepare; `Activate` |

---

## 9. Cells and committed values

### 9.1 Cell carry at entry

At entry (§7.2 step 4), each continued cell whose value changed is applied as a `SetCell` from the old
value to the new would be (RENDER-22a): it ramps over one block if its control is smooth, and steps
otherwise.

- **Ramps in flight end first.** A ramp written at the old plan's last block line is still in its
  constants' buffers, with their version at `COPY`. §7.2 step 0 ends the old plan's ramps before any
  box moves, exactly as its next block line would have. Without it, a kept poly's voice graphs would
  carry a `Ramp(from, to)` buffer that every voice kernel copies each block: a 375 Hz sawtooth on the
  parameter until the next write to that cell. Top-level cells are unaffected either way, since their
  const nodes are plan-side, but the same step covers them.
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

### 9.3 The value fast path stays

The server's fast path (`same_layout`, `value_writes`, `Reach.cells`, the cell-write branch of
`Way::Reach`, and `Open::adopt`'s re-key) stays as built: a value-only revision is a cell write, with
no prepare and no plan. Cell carry covers the value part of a mixed edit. No gate needs the fast
path's deletion, and the data-plane workflow's faster Tx path may build on it, so step 7 doesn't touch
it. The lease rule "the engine installs the committed value in the same step that ends the lease"
(WRITE-22) holds, because a renderer built while a lease lives gets its value before it is sent, as
today.

---

## 10. Plugins

### 10.1 Persistent instances

Plugin instances move like every box (§7.2): one with a state path by its module and path, any other
by name, and both by `Shape::Plugin`. This replaces `CarryKey`, whose ordinal matching hands an old
instance's state to a newly inserted unkeyed node.

- **No plugin branch inside voices.** The checker refuses plugins inside `poly` (`check.rs:563`), so
  the runtime's `in_voice` branch is deleted.
- **A changed state blob:**
  - through slice 3, the shape includes the state hash, so the instance is new, exactly as today;
  - slice 4 removes the hash from the shape and loads the state into the kept instance (§10.2).
- **Last-sent values continue, safely.** `plugin_events` (`core/runtime/src/lib.rs:314`) sends a
  `Port` parameter whenever its sample differs from the last value sent, but an `Input::Const` only
  when nothing was sent yet (`if last.is_none()`). A plain number passed to `plugin()` is folded into
  `Input::Const` by the checker (the const-folding branch near `check.rs:708`), so a kept instance
  whose constant changed from 0.7 to 0.5 would carry `Some(0.7)` and never hear 0.5. The `Const` arm
  becomes `if *last != Some(v as f32)`, as the `Port` arm already is. With that one line, a copied
  last value can never hide an edit, and a no-op activation still sends nothing. It lands in slice 1;
  offline it changes nothing, since `last` starts at `None` and then equals the constant. With-plugins
  already exercises it: `tracks/lead.ts` passes Airwindows `dry_wet_2944917344: 0.35`, a plain
  number, and the gate's "code edit inside the lead" check edits it to 0.9 and reads it back from the
  kept instance (`crates/gates/src/plugins.rs`, about line 700).
- **`keys` are never cleared on a load.** An Off for a note the plugin has forgotten is harmless, and
  a rewired consumer must still release what it holds.

### 10.2 State changes (slice 4): a load inside prepare, not an engine message

For each kept plugin, prepare compares the committed blob of its state path in the new `DataSet`
(`DataSet::state`, verified by the snapshot) with `I.plugins[j].blob`. When they differ:

1. **A removed blob** (old `Some`, new `None`): the node is not kept (§6.2), since CLAP has no "load
   defaults"; a fresh instance is created.
2. **Otherwise,** `host.load_state(h, blob)` on the plugin main thread, as the last step of prepare,
   after every fresh instance exists, so a prepare that fails does so before any load. It is skipped
   when the blob's hash equals the state the instance *holds* (HOME-6). CLAP's `state.load` is a
   main-thread call allowed while the plugin is active.
3. `inst.reloaded.store(true)`, a new `AtomicBool` on `PluginInstance`. At its next call, in the old
   plan or the new one, the node swaps it to false and clears every last-sent value, so every
   code-owned parameter is sent again in that call (HOME-4: code beats the blob). The blob's value
   plays for less than one block.
4. `host.latency(h)`. A nonzero latency raises the Warning `plugin.latency-uncompensated`, naming the
   plugin. Compensation doesn't exist, and the check refuses latency at default state (RENDER-21), so
   there is nothing to re-plan.

Around it:

- **The comparison is per node, not against `GraphDelta.states`,** which lists a path only when its
  blob file changed (`core/session/src/tx/delta.rs:554-562`). So a pairing across paths (a renamed key
  matched by name onto a path that has a blob) loads the right blob although no file changed.
- **A state-only revision is an identity `Activate`** (§8.3). The server and the engine rig
  (`crates/gates/src/engine.rs`) both call `prepare`, so both get the loads from one place.
- **The host fix,** in `core/plugins/src/inprocess.rs` `load_state` (`:275`). Rename `last_saved` to
  `holds`, and set it on every successful load (`create` with a blob included) as well as every save.
  - Without it, save S, then load C (an undo), then load S (a redo) skips the last load.
  - With it, the export's load of the committed blob is skipped in the usual case, where the instance
    holds it already.

### 10.3 Restart (`HostEvent::Restart`, slice 2)

1. The actor's owner events put the handle in `Open.restarts: HashSet<Handle>`, which replaces
   `Carry::restart` and `PluginInstance.restart`.
2. It settles at once with an identity activation. The marked instance doesn't match (§6.2), so a
   fresh one is created from the committed blob.
3. The fresh one fades in while the old one rings out as a ghost for 5 ms.

Until plugin windows ship, an instance's state is its committed blob plus its code-owned parameters,
which the fresh node sends at its first call, so nothing is lost and no `save_state` round trip is
needed. One exception (the step 7 review's plug-1): a parameter the code stopped setting keeps the
value it was last sent (HOME-5a), which no file records, and a restart takes it back to the blob's
value. Today waits for the next rebuild and loads the blob from disk.

A plugin that fails live (a processing error, or no activate after an export) takes the same path:
its node plays silence and never calls it again, and the session puts it in `restarts` (review fix
plug-2).

### 10.4 "loading <plugin>" (RENDER-25, slice 4)

While `prepare` creates plugin instances, the actor publishes `loading: [plugin names]` in the engine
view through the control plane, set before each `create` and cleared after. The web shows "Loading
Surge XT…" in its status area. The old plan plays meanwhile, and the 200 ms budget doesn't count it.

### 10.5 Export: hold, borrow, render, resume (RENDER-1, slice 4)

A song whose export plan has at least one plugin pauses playback. A pure song, or one whose plugins
are all missing, exports beside playback as today.

**`Hold` in the engine** (`core/runtime/src/engine.rs`):

- `Hold(true)` applies once no window runs. For `fade` frames the engine renders normally with its
  output scaled linearly to 0. From the next block on it renders nothing:
  - no kernel, plugin or scheduler runs;
  - the engine clock doesn't move;
  - it outputs silence;
  - `Status.held = true`.
  This is RENDER-6a's one exception, added to that rule in slice 4.
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
  output ramped linearly from 0 over `fade` frames. This is a tape pause (owner question Q6).

**The actor** (`crates/server/src/export.rs` `start`, and `Open.exporting`):

```
pure song (no FOp::Plugin in order): as today, a Renderer on its own thread, beside playback.

otherwise:
  actor: exporting = true          settles publish the view but prepare nothing and send no
                                   Activate, so no state load or other host call reaches a live
                                   instance; SwapLane, cell writes and transport go on
  map the accepted revision's plugin nodes to live instances:
      continues(plays.revision, accepted), equal shape -> PlanInfo.plugins' Arc
  export = Render::borrowing(accepted, Basis::Borrow{..})
      kernels fresh (RENDER-1); borrowed nodes flagged reset_first; other plugins created, destroyed after
  player.hold(true); wait up to 2 s for player.held()
      not held (the device stalled): player.hold(false), export with fresh instances, Notice says so
  spawn music2-export, which does:
    for each borrowed h, on the plugin main thread:
        deactivate(h)
        load_state(h, committed blob)            // when there is one; skipped when it holds it (usual)
        set_render_mode(h, Offline)
        activate(h, rate, BLOCK)
    render on this thread: at a reset_first node's first call, host.reset(h)
        (CLAP's reset needs an active instance; code-owned parameters go at the first call, as the
        export plan's last-sent values are None)
    finally (success, error, stop, or a caught panic), a guard dropped on every path:
        for each borrowed h: deactivate(h); set_render_mode(h, Realtime);
                             activate(h, rate, BLOCK); inst.reloaded = true
        drop the export renderer (its Arcs go; the live plan still holds the instances)
        player.hold(false)
        post ExportDone: actor exporting = false, settle once
```

**Nothing is saved or restored.** After the render each instance holds the committed blob, which is
the state it had, and `reloaded` sends its code-owned parameters again at the first resumed block.
That holds because `PlanInfo::lend` borrows only an instance that holds its node's committed blob and
the parameters its code sets. A parameter the code stopped setting keeps the value it was last sent
(HOME-5a), which no file records (the step 7 review's plug-1), so such an instance isn't borrowed:
its node plays a new instance from the committed blob, as `m2 render`'s does. Neither is one whose
plan never loaded its node's committed blob (a build that failed since, plug-3), one that failed, or
one waiting for a restart (K7). So an export never changes what a live instance holds, and the file
depends on the project's files alone, not on the edits that led to them. HOME-6's "a saved state is never
pushed back into the instance that produced it" holds, and the pause costs no second patch load per
instance (tens of ms for Surge XT, seconds for a sample-based instrument). When plugin windows ship,
an export must first commit a pending autosave, so the committed blob is again what the instance holds
(RENDER-1's edit says so).

**Failures:**

- A failed host call on one instance before the render makes that node fresh for the export, and it
  goes in the export's `problems`.
- A failed reactivation after the render marks the instance failed before playback resumes, so its
  node plays silence and never calls it; the session puts it in `restarts`, and the settle after the
  export replaces it from the committed blob (§10.3, review fix plug-2).
- Quitting during an export takes the same guard path.

**What the producer hears and sees:**

- the output fades out over 5 ms, and the status says `paused: "exporting"`;
- the transport resumes where it was, playing if it was;
- kernel tails resume where they froze;
- plugin voices were reset by the export, which is RENDER-1's stated cost;
- plugins come back with their state intact (the gate).

**CLI.** `m2 render`, `m2 bounce` and agent range renders keep creating their own instances: the CLI
has no live engine.

**Render mode** (review fix K4). Every render that isn't live sets each new instance to CLAP's offline
mode on the plugin main thread before activating it: `m2 render`, range renders, an export's own
instances and the one it makes for an instance it can't borrow. The live engine's stay in realtime
mode, CLAP's default. A borrowed instance goes offline for the export and back to realtime after it.

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

The gate measures the runtime itself, with no server and no device. It uses only APIs HEAD already
has (`Renderer::new`, `render_block`, `Drop`, and the `Flat` for counting), so the same file runs in a
worktree at HEAD and on slice 1:

| Layout | What | When |
|---|---|---|
| **S** | HEAD's one Store per renderer: the baseline | recorded once, from a worktree at HEAD, before slice 1's first commit (sharing the main target dir, and removed after) |
| **P1** | slice 1's one Store per instance, default tunables (4 GiB reservation plus guards) | slice 1 |
| **P2** | one per instance with `memory_reservation(KERNEL_MEMORY)`, `memory_guard_size(64 KiB)`, `memory_reservation_for_growth(0)`: explicit bounds checks | only if P1 fails, as `wasmtime::Config` options on `PackageSet`'s engine |
| **P3** | one per instance with the pooling allocator (wasmtime feature `pooling-allocator`): `total_memories` and `total_core_instances` at 2 × count, `max_memory_size = KERNEL_MEMORY` | only if P1 fails, the same way |

Measured per layout:

1. **Count.** Top-level kernel nodes, plus, for each `poly`, voices times its voice graph's kernel
   nodes, from the `Flat`. It prints the count beside the contract's 6,176 (the runtime notes count
   about 6,060 from std's sources), and acceptance gate 7 takes the measured number.
2. **Build:** `Renderer::new` on p50, and the time per instance (total over count).
3. **Memory:** `task_info` `phys_footprint` before and after; address space reserved (the
   `mach_vm_region` sum) against the process limit; the largest number of p50 renderers that build side
   by side, up to 4.
4. **Block time:** `render_block` over 11,250 blocks (30 s) from 3:00: mean, p99 and max.
5. **Drop:** the renderer dropped on another thread, while an audio-like thread renders a small song
   in 2.67 ms periods: the longest drop, and that thread's p99 during it.

The leak control of revision 1 is gone: a long-lived store's growth is the churn test's job (§12.7),
on the real arena.

### 11.3 The decision rule, stated before measuring

**Choose P1, unless one of these fails against S:**

- p99 block time worse by more than 80 µs, which is 3% of a 2.67 ms block;
- footprint worse by more than 64 MB;
- the build fails below 2 renderers side by side, or reserved address space at 2 × the count exceeds
  25% of the process limit;
- p50's full build (opening a project) worse by more than 50%.

**If P1 fails,** measure P3, then P2, and choose the first that passes all four (P2's bounds checks
show in the block time). If none passes, step 7 stops and the owner decides. S and per-batch stores are
never chosen.

**Batched drops** are built only if the chosen layout's drop raises the audio-like thread's p99 more
than dropping S's single store does. That drop is today's cost on every edit.

**After slice 2,** G7f's second part plays p50 at 3:00 for 30 s with the arena, in a recorded release
run, and applies the same rule to the real numbers:

- the p99 callback with the arena against HEAD's, within 80 µs;
- the activation block's callback time over G7a's ten edits: the slowest under half the device
  buffer's period (5.3 ms at 512 frames and 48 kHz), printed against the run's p99 callback.

It writes both into RENDER-32's Today line.

---

## 12. The gates, and how each is measured

Timed gates run in a recorded release run on the reference Mac (M1, 48 kHz, 512-frame device
buffers). The rest run in CI.

**Rig changes** (`crates/gates/src/engine.rs`):

- `Song` carries its `Arc<Revision>`, so the rig calls `continues` exactly as the server does.
- The rig prepares with `Basis`, so it gets clip renders, audio swaps and (slice 4) state loads from
  `prepare`, as the server does.
- `--no-carry` becomes `--no-arena`: `Basis::Keep` with an empty `Continues`, so everything is fresh
  and the ghosts are the whole old graph. `--keep` already names "keep the work folder". This is the
  negative reference for every arena gate.
- `Probe` gains the last activation's `Stats` (kept, fresh, ghosts, fades, resets), plus `refused` and
  `leaked`.
- `Renderer::output_of(node) -> &[f32]` returns the last block's buffer of a node, looked up through
  `PlanInfo.index`. It is gates-only reading through `Driver::engine()`, with no audio-thread code.

### 12.1 G7a: "In p50, a structural edit at 3:00 is audible within 200 ms after the compile"

- **Server gate:** `m2-gates --server --reference 50 --at 180 add-effect`.
  - `--at` is a new `Opts.at`, default 60; `open()` seeks there before playing.
  - Ten edits, alternately adding and removing a `delay` on `t02` through the app's `add_effect` Tx.
- **Pass:**
  - every `heard_ms`, counted from `requested_ns`, is at most 200. That includes the compile, check
    and verify, so it is strictly harder than the contract's "after the compile finishes", which stays
    as written;
  - no jump back (`Sampler`);
  - G6's step bound holds across each edit;
  - at most 3 fresh instances per edit, from slice 3 on.
- **Printed:** the parts, from `EditTiming` plus a new `prepared_ns`: the session's request (compile,
  check, verify), `continues`, prepare, the wait for the block, and one device buffer; and `heard_ms`
  from `compiled_ns`, the contract's own start.
- **Expected:** about 84 ms of compile (PLANE-4) plus 20 to 40 ms (*estimate*), against 150 to 170 ms
  for HEAD's build alone after the compile.
- **Engine-rig twin:** `gates::add_effect --reference 50 --start 180` (in place of `one_minute`), which
  keeps measuring what the audio thread pays.

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
- the activation's `Stats` say every stateful node of the 49 tracks and of the mixer was kept, and
  none was reset;
- `resets` is unchanged and `held_notes` agrees at the end.

**Printed, not judged:** the activation block's callback time against the median callback. Its budget
is G7f part 2's (§11.3), in a recorded run, so a busy CI runner can't fail a bit-exactness gate.

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
     fresh, 0 ghosts, 0 fades and 0 resets.

  This covers schedulers, voice pools, rings, audio players, cell carry and the wrappers' first-call
  copies.
- **New variants:**
  - `an_activation_with_rebuilt_lane_data_changes_no_bit`: every lane's data rebuilt (new `Arc`s,
    equal contents) and sent in the lane moves. Audio lanes crossfade through `xfade`, whose equality
    branch keeps them exact, so no `Arc`-identity shortcut is needed; `examples/resample` is in the
    set;
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
- **State intact:** each instance's `save_state` bytes before the export equal those after the first
  resumed block.
- **Resumes:** playing at the same transport frame within two buffers of `done`, with no step over G6's
  bound at the resume.
- **The file:** a WAV of the song's length. Plugins aren't bit-exact across instances, so its RMS per
  second and onsets are compared with `m2 render`'s, within 1 dB.

**The CI test `an_export_borrows_and_gives_back_the_live_instances`** (`crates/server/src/graph/tests.rs`)
uses the probe plugin (`core/plugins/src/probe.rs`, `clap:m2.probe`). The project has a saved state,
and code sets a gain.

- The main-thread call log is, in this order: `deactivate`, `load_state` (committed; the host reports
  it skipped, since the instance holds it), `set_render_mode(Offline)`, `activate`; then after the
  render `deactivate`, `set_render_mode(Realtime)`, `activate`. No `save_state` and no second
  `load_state`.
- `reset` is called on the export thread after the first `activate`.
- No `process` call comes from the audio thread between the first `deactivate` and the last
  `activate`.
- The borrowed export equals a fresh-instance export bit for bit. The probe is deterministic, so this
  proves `reset` plus the held committed state give RENDER-1's exact start.
- After the resume, the probe's gain parameter reads the code's value (`reloaded`).
- Negative control: with the hold removed (a test-only flag), the probe sees audio-thread `process`
  calls inside the window.

**A pure song** (`examples/first-song`) exports beside playback: no hold, no state call.

### 12.6 G7f: "The per-instance Store cost at 6,176 instances is measured, and the store layout is chosen from it"

See §11. The report prints the measured count, the table of layouts against metrics, the rule's
outcome, and, after slice 2, part 2's numbers.

### 12.7 The churn test (no memory growth)

`random_structural_edits_leak_nothing`, in `core/play/tests/live.rs`, is a CI test:

- on `examples/first-song`: 50 structural edits while playing (wraps, unwraps and replacements on
  rotating nodes), plus 5 tempo changes;
- on `examples/resample`: 10 audio clip edits (gain, fade, a moved clip) while playing;
- after draining the dropper: `LIVE_KERNELS` and the host's `live()` equal their values before the run,
  `Engine.leaked == 0`, and every clip render the playing plan no longer uses is freed (the test holds
  `Weak<RenderedClip>`s from a test hook on `audio::Cache` and checks that they no longer upgrade).

A recorded p16 run (200 edits) also prints the footprint, and must stay within +20 MB.

### 12.8 Gates that keep passing

- Every step 3 gate: G1 to G7, G10, G11, G8 `previews` and G9 `drag`.
- With-plugins: its "carried instances" checks become "kept by the arena" with the same assertions
  (`same_instances` in `crates/gates/src/plugins.rs`: `created()` unchanged, notes go on).
  - `new_state` (`plugins.rs:728`) keeps asserting "a new Surge, Airwindows carried" through slice 3.
    Slice 4 rewrites it: the same Surge handle, `created()` unchanged, exactly one `load_state` on the
    main thread, Surge's volume parameter reading the new patch's value, and a second commit of the
    same blob skipped (HOME-6). The engine rig passes it because its settle calls `prepare`.
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
  - `a_box_that_went_non_finite_is_reset_when_it_moves`: a WAT test kernel that turns its state NaN on
    cue, read by a WAT kernel that maps NaN to 0; an edit on another track resets the first kernel,
    keeps every other instance bit-exact, and the track sounds again;
  - `a_transient_inf_keeps_every_instance`: one inf sample at a stateless kernel's output, then an edit
    on another track; `Stats` reports 0 resets and every other tap is bit-identical;
  - `a_trapped_kernel_is_reset_at_the_next_activation`;
  - `panic_clears_every_flag`.
- RENDER-25a:
  - `an_inserted_effect_fades_dry_to_wet`, which checks the window's samples against `xfade`;
  - `a_removed_effect_rings_out_as_a_ghost_then_goes`, where `Retired::Nodes` holds it at the
    window's end;
  - `a_silent_edit_changes_no_bit`, the equality branch;
  - `an_effect_reorder_replaces_the_cycle_and_keeps_the_rest`;
  - `a_ghost_reading_a_removed_control_lane_reads_its_ghost_lane`;
  - `feedback_rings_survive_an_activation`, with no 128-sample hole.
- RENDER-26a and RENDER-9:
  - `a_replaced_instrument_releases_its_notes`;
  - `a_kept_consumer_keeps_its_notes` (replaces `a_carried_instance_keeps_its_notes`);
  - `a_rewired_poly_is_kept_and_releases_its_notes` and `a_rewired_plugin_lets_go_of_its_keys` (F8);
  - `a_lane_that_outgrows_its_room_grows_without_a_restart`;
  - `note_ids_never_repeat_into_a_plugin` (replaces `ids_never_repeat_after_a_renderer_without_notes`);
  - `a_jump_on_the_activation_line_ends_its_notes`: a Stop, a Seek and a block-line wrap (a 4-bar loop
    at 120 bpm and 48 kHz, 3,000 blocks) on the activation's line; `held_notes() == 0` after the Stop,
    every voice's `note.off` set, and the moved audio player's output within G6's step bound;
  - ported with the same assertions: `a_fresh_instance_chases_nothing`,
    `a_lane_that_shrank_lets_go_of_what_doesnt_fit`, `notes_the_edit_removed_are_let_go`.
- RENDER-27:
  - `a_tempo_change_keeps_every_kernel_and_the_beat`, with an unwarped clip after the change point
    whose onset sample is checked under the new map;
  - `a_tempo_change_recomputes_pending_offs`;
  - `an_audio_clip_edit_crossfades_and_keeps_the_rest` (on `examples/resample`);
  - `an_audio_swap_of_unchanged_clips_changes_no_bit`: one clip's gain edited while another clip of
    the lane plays; the playing clip is bit-equal through the window;
  - `a_clip_gain_edit_stays_between_its_two_levels`: a +1 dB edit; every sample of the window lies
    between the two sides, and the window's RMS between theirs.
- RENDER-23 and RENDER-22a:
  - `a_committed_smooth_value_ramps_through_an_activation`;
  - `a_kept_polys_voice_cells_take_the_new_values` (a write counter: only the differing ones), with a
    ramp written one block before the activation: the voice input is flat on every block after it;
  - `a_lease_value_set_before_the_send_plays_from_the_first_block`.
- HOST-5: `a_changed_constant_parameter_reaches_a_kept_plugin` (the probe; 0.7 → 0.5 is sent once).
- RENDER-28: `activations_are_never_coalesced`.
- Panic: `panic_during_a_window_resets_ghosts_and_ends_on_time`.

**`core/runtime/src/sched.rs`, `audio.rs`, `transport.rs`, `lib.rs` unit tests:**

- `take_state_keeps_sounding_notes`;
- `a_swap_reads_the_old_renders_at_their_own_position` (`shift`);
- `one_player_per_lane_gives_the_old_bits` (two nodes on one lane, against two `AudioNode`s);
- `a_jump_on_an_entry_line_wins` (`Transport::advance`);
- `a_changed_constant_is_sent` (`plugin_events`);
- the existing swap, wrap and ordinal tests unchanged;
- the adoption and `release_all` tests deleted with what they test.

**`core/play/tests`:**

- `bit_identity.rs`: G7c's additions.
- `no_alloc.rs`: one activation under the allocation trap (which counts deallocations too), with cell
  writes from another thread, that:
  - moves kernels, polys, two probe plugins, rings and audio players;
  - grows a lane;
  - swaps an audio lane;
  - makes ghosts, ghost lanes and fade nodes;
  - releases notes;
  - resets a poisoned box at entry;
  - ends a window and retires the leftovers and a player's old renders;
  - refuses a stale activation;
  - panics mid-window;
  - holds and resumes, with a deferred panic.

  It also asserts that `kernel_memory_bytes` is unchanged for the kept instances.
- `live.rs`:
  - `no_edit_or_jump_leaves_a_note_stuck` and `random_edits_and_jumps_never_strand_a_gate`, extended
    with random structural edits between jumps, and with jumps placed on activation lines;
  - the churn test (§12.7);
  - `a_hold_freezes_and_resumes_without_a_step` (RENDER-6a's exception, RENDER-1).

**`core/session`:**

- `continues` (slice 2): `an_insertion_above_continues_everything` (step 6's gate edit),
  `a_label_rename_continues_everything` (ID-10), `untouched_files_continue_whatever_changed`, and
  `keyed_plugins_follow_their_paths` (ID-8; the port of `keyed_instances_follow_their_keys`): two keys
  swapped in place, `["lead","pad"].map(…)` reordered to `["pad","lead"]`, and with-plugins' keyed
  Surge wrapped in an effect.
- `continues` (slice 3): `a_loop_that_grows_resets_its_occurrences` and
  `a_keyed_loop_keeps_through_reorder_and_growth` (ID-14), `adding_a_plugin_key_continues_its_instance`
  (ID-8: the name pass after the path pass).
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

- `a_reload_after_another_load_is_never_skipped`: save S, load C, load S, and S loads;
- `a_load_of_the_held_state_is_skipped` (HOME-6), including a blob given at `create`;
- `a_state_load_resends_code_owned_parameters` (HOME-4).

**`crates/server/src/graph/tests.rs`:**

- `an_audio_edit_is_an_activation_that_keeps_every_instance` (slice 2);
- `a_tempo_change_keeps_every_instance` (slice 2);
- `a_restart_request_replaces_the_instance_from_its_committed_state` (slice 2);
- `a_refused_activation_recovers_by_replace` (slice 2);
- `swapping_two_plugin_keys_moves_the_instances`: in slice 2 with equal or absent blobs, `created()`
  unchanged; in slice 4 with distinct blobs, `created()` unchanged and each instance's `save_state`
  follows its key;
- `a_state_change_loads_into_the_running_instance` (slice 4; `created()` unchanged);
- the G7e probe test (slice 4);
- `settles_during_an_export_wait_then_apply_once`, `lane_previews_play_during_an_export` (slice 4);
- `loading_is_published_while_a_plugin_is_created` (slice 4);
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
| `commands_land_on_the_block_line_as_one_jump` (`transport.rs`) | its last assertion, `enter()` then Play on one line, expects `Start`, not `Enter`; `adopted_notes_go_on_through_enter` and the activation trace test in `tests/live.rs` have no other jump on the line and are unchanged | RENDER-9, RENDER-26a |
| gates `--no-carry` | becomes `--no-arena`, the negative reference for G7b | RENDER-25 |
| gates `new_state` (`crates/gates/src/plugins.rs:728`) | slice 4: the same Surge, one load, the new patch's volume, a repeated blob skipped | RENDER-26b, HOME-6 |

---

## 14. What this deletes

| Deleted | Where | Replaced by |
|---|---|---|
| `CarryKey` (with its ordinal and state hash), `Carry` (`none`, `of`, `source`, `handles`, `restart`), `InstanceSource`, `Fresh` | `core/runtime/src/plugins.rs` | `continues` (path pass and name pass), `Shape` and moves; `Basis` |
| `carry_from`, `lend`, `Renderer::carry_from`, `render_block_lent` | `plugins.rs`, `lib.rs` | `take_over` |
| `PluginInstance.{key, restart, held_cap, next_id}`, `key()`, `hold()`, `mark_restart()` | `plugins.rs` | `Shape::Plugin`, the actor's `restarts`, engine-wide ids |
| `PluginNode.lent`, `PluginView.lent` | `lib.rs` | nothing: no instance is processed twice |
| `Builder.ordinals`, `Builder.source`, the `in_voice` plugin branch (`ordinal = u32::MAX`) | `lib.rs` | moves; the checker already refuses plugins in `poly` |
| `Renderer::build_with`, `build_live`, the per-renderer `Store` field | `lib.rs` | `Renderer::prepare(.., Basis)`; a `Store` per `KernelInst` |
| the `store` parameter on `Graph::{process, reset, prewarm, active, kernel_memory}`, `PolyNode::process`, `KernelNode::{process, reset}` | `lib.rs` | each instance's own store |
| `Exec::Capture(PortRef)` with outer indices inside voices | `lib.rs` | `Exec::Capture(u16)` into `PolyNode.captures` |
| `Exec::Audio(Box<AudioNode>)`, `AudioNode` as a graph node, its "nothing is faded at a renderer's first block" case, and its `Enter` arm's clearing of `fade_in` | `lib.rs`, `audio.rs` | `AudioPlayer` in `LaneSlot::Audio`; `Exec::Audio { lane }` |
| `render_block`'s `release_all` parameter | `lib.rs` | persistent schedulers; `Entry.release` |
| `NoteScheduler::{adopt, raise_next_id, next_id}`; fields `next_id`, `released`, `adopted`; `run`'s `release_all` | `core/runtime/src/sched.rs` | persistent schedulers; `Renderer.next_note`; `take_state` for growth |
| `Enter`'s priority over every other jump | `core/runtime/src/transport.rs` | real jumps first (§8.1) |
| `struct Fade`, `Engine.fading`, `fading()`, the two-renderer mix, `heard[1]`, the fading arms of `apply_now` and `resets()`, the coalescing branch | `core/runtime/src/engine.rs` | `Exec::Fade` and ghosts inside one plan |
| `Open.carry`, the `Carry` in `Way::{Activate, Replace, Load}`, `build()`'s `InstanceSource`, `Carry::restart` in `heard_plugins` | `crates/server/src/graph/actor.rs` | `Plays.info`, `Basis`, `Open.restarts` |
| `Plays.{slots, caps, cell_slots, cells}` | `actor.rs` | `PlanInfo` |
| a whole rebuild for an audio-lane, asset, state or arrangement change (`reach`'s `None` today) | `actor.rs` | an identity `Activate` that keeps every instance |
| `Lab.carry` (`--no-carry`), the rig's `Carry` and `carry_epoch` | `crates/gates/src/{lib,engine,main}.rs` | `--no-arena`, `Basis` |
| `tests/live/carry.rs`; `sched.rs`'s adoption and `release_all` tests | `core/runtime` | `tests/live/keep.rs` |
| (slice 3, optional) `carry_text`, `carry_ends`, `carry_lines` | `crates/editor/src/controls.rs` | `text::Edits::between(..).carry` |
| (slice 3) `m2_ir::control_id`, which only a test uses | `core/ir/src/lib.rs` | the control constant's delta name |
| Contract concepts: ID-13's numbered `SiteId(u32)` and its rule 4 (to Deferred); the engine shape's `arena: Map<RuntimeId, Slot>`; the `SetArrangement` and `LoadState` messages; RENDER-28's coalescing; RENDER-32's per-partition fallback; RENDER-1's "given back the state they had"; step 3b's clarification C7 | `docs/architecture.md` | §15 |

**Never built** (revision 1 concepts removed before any code): `Renderer.tainted`, `PlanInfo.tainted`
and `Basis::Keep.wipe` with their special cases; `LaneSwap::Audio` on the `SwapLane` message path and
its readiness rule; `Tap.src` and the re-pointing of taps at a swap; F7; the export's `before` save and
restore; restart's save round trip; §9.3's conditional deletion and its value-edit gate; G7f's
wasmtime re-implementation, its T layout and its leak control.

**Unified:**

- **One `Activate` with moves** carries: a code edit, a tempo or meter change, an audio-lane or asset
  edit, a plugin state change (slice 4), a notes lane that outgrows its room, a plugin restart, and a
  package change.
- **`SwapLane` is unchanged:** notes and control lanes, with one address, `(generation, slot)`.
- **One way to move:** kernels, voice pools, plugins and rings move by `take_from`, lanes by `LaneMove`,
  all by swap.
- **One crossfade function,** `xfade`, for edge fades and audio swaps; equal-power only for jumps.
- **One window:** fade nodes, ghosts, ghost lanes and audio swaps all end at `end_fade`, which retires
  them together, so one readiness check ("no window running") covers them all.
- **One mechanism** gives a removed node's ring-out and a replaced note consumer's release: a ghost.
- **One reset rule:** `Panic` resets every box; an activation resets each moved box that went
  non-finite or trapped.
- **One place for state loads:** `prepare`, which the server and the engine rig both call.
- **Export's reuse is activation's match,** sharing `Arc`s instead of moving boxes.
- **One activation fade mechanism** (edge fades). `LiveFade` in `core/play` stays only for replacing a
  whole engine: opening a project, a rate change, a refused activation's recovery, closing.

**Kept on purpose:**

- `LiveFade`, and `Renderer::enter_from` (it serves `take_over` and `Replace { keep: Position }`);
- `Player::send`'s 500 ms `Stalled`;
- `Renderer::seek`'s offline replay (RENDER-1);
- `Jump::Enter`, for a line with no other jump;
- the `mem::forget` fallback, now counted;
- the server's value fast path (§9.3);
- `delta_between` and `reach`, unchanged.

---

## 15. Contract edits

Each lands in the slice that changes the code (the contract's own rule: "A change that breaks a rule
changes this page first, in the same commit").

| Rule | Now (in short) | Proposed (in short) | Why | Slice |
|---|---|---|---|---|
| Planes table: Prepared data, Arrangement and Code rows | prepared data: `SwapLane`, `LoadState`; arrangement: `SetArrangement`; code: "new plan; Activate" | prepared data (audio lanes, assets, plugin state): "clip renders and state loads prepared off the audio thread; an `Activate` of the same structure that keeps every instance"; arrangement: "tables and clip renders prepared off the audio thread; an `Activate` of the same structure"; code: "compile, check, verify, prepare against the plan that plays; `Activate`". Budgets unchanged | an activation that keeps everything is a data switch: one path, one test; it is the path RENDER-27 already names | 2 (state: 4) |
| RENDER-1 | calls: `deactivate`, `reset`, `load_state`, `set_render_mode(Offline)`, `activate`; "given back the state they had" | "The engine is held first: it fades out over 5 ms and processes nothing, which it publishes before any host call. Each reused instance is deactivated, loaded with the committed blob (skipped when it holds it, HOME-6), set to offline mode and activated, and is reset on the render thread before its first `process`. Afterwards each is deactivated, set to realtime and activated, and is left holding the committed blob, with its code-owned parameters sent again at the next block (HOME-4). The engine resumes where it held, fading in over 5 ms. Once plugin windows ship, an export first commits any pending state autosave (HOME-6)." Today: the `Hold` | CLAP's `reset` needs an active instance; the held blob is the state the instance had, and loading a saved state back is what HOME-6 forbids | 4 |
| RENDER-6a | "MUST keep processing while the device is open" | add: "Except while held for an export with hosted plugins (RENDER-1): the engine then processes nothing, and resumes where it held." | `Hold` freezes kernels and the clock | 4 |
| RENDER-10 | as is | add: "A plan activation never resets a kept instance, except one whose output went non-finite or that trapped (RENDER-25)." | states the arena's guarantee | 2 |
| RENDER-16 | "A voice's RuntimeId is its allocator slot." | "A `poly` is kept or replaced whole; a kept pool's voices keep their slots, which are their RuntimeIds." | 97% of instances are voices; one box keeps entry O(top level) | 2 |
| RENDER-22 Today | the server's guess (`same_layout`) decides cell write or new renderer | "A committed value reaches the engine as a cell write when only constant values changed (`value_writes`); with other changes, in an activation that keeps every instance and applies each changed continued cell as a `SetCell` from its old value would be (RENDER-22a)." | as built | 2 |
| RENDER-23 Today | "cells live and die with their renderer until the arena keeps them across plans (step 7)" | "Cells stay per plan. Leases are re-aimed at each activation; a continued cell whose value changed is applied at activation as a `SetCell` would be (RENDER-22a), after the old plan's ramps end." | cells never needed to persist | 2 |
| RENDER-25 | "keeps every instance whose RuntimeId and shape match"; shape listed | add: "Matched when the plan is prepared, off the audio thread, against the plan the engine will be playing; `Activate` carries the moves and names that plan. The audio thread checks that plan's id and each move's shape before moving anything, and refuses the whole activation on a mismatch. A kernel's module hash is the SHA-256 of its package's wasm module, which also fixes its state ABI; in ABI v0 its only setting is its channel count. A `poly`'s shape is its voices, channels and voice-graph structure without cell values. An instance whose output was non-finite in its last block, or that trapped, is reset as it moves." The budget sentence is unchanged. Today rewritten, including the stale retired-queue text | where matching runs, what makes stale memory impossible, and how a broken instance heals | 2 |
| RENDER-25a | "old and new instances run together for one fade window … at least at every module output whose subgraph changed … Step 3 … crossfades the whole output" | "An activation crossfades, linearly over 5 ms, every input of a kept node whose source changed, and the song output when its source changed. A replaced or removed node runs on in the new plan for that window as a ghost, its notes ended, reading what it read, then is retired. Nothing else fades; equal signals stay bit-identical. An edit that would make that window's graph cyclic (a reorder of kept nodes) replaces the nodes on the cycle." | fading at module outputs needs kept instances processed twice (lending); edges need neither | 2 |
| RENDER-26 | as is | no edit to the rule; its Today line gains "audio lanes and assets take RENDER-27's path" | `SwapLane` is unchanged | 2 |
| RENDER-26a | "An activation that replaces a note consumer first sends an Off…"; `SetArrangement` recomputes Offs; Today: adoption, C7 pending | "Each notes lane's scheduler persists across plans with its sounding notes, keyed by `LaneAddr`, and note ids are unique across the engine. An activation that replaces a note consumer, or keeps one whose notes input now reads another lane, first ends every note sounding into it. A stop, seek or wrap on an activation's block line is handled as on any block. An arrangement switch reconciles every scheduler with its new data, which recomputes each pending Off from its beat." Delete C7's paragraph; the clarification is withdrawn | adoption existed only because schedulers died with their renderer | 2 |
| RENDER-26b | "MUST reach the running instance as `LoadState` … last saved … a plan-only activation re-plans the compensating delays" | "…as a state load on the plugin main thread into the instance that plays, made while the plan for that revision is prepared; skipped when its hash equals the state the instance holds (HOME-6); its code-owned parameters are sent again at its next block (HOME-4). A removed blob makes the instance new. Until compensation ships, a latency that becomes nonzero raises `plugin.latency-uncompensated` and plays uncompensated." The asset sentence stays | CLAP's state load is a main-thread call; there is nothing to re-plan while RENDER-21 refuses latency | 4 |
| RENDER-27 | "…then switch them together at one block boundary" | add: "The switch is an `Activate` of the same structure, which keeps every instance. Each audio lane with new renders crossfades from its old renders linearly over 5 ms, exactly where the two are equal. A tempo change re-renders every clip, warped or not, since the tempo map places and sizes each one." | the same path as code; a tempo change can't cut a tail or leave audio off the grid | 2 |
| RENDER-28 | messages `(Activate, SwapLane, SetArrangement, LoadState, Transport)`; "the sender waits … or coalesces to the newest plan" | messages `(Activate, SwapLane, Transport, Panic)`, and `Hold` from slice 4. "Activations are never coalesced: each applies after the previous one's fade window, against the plan it names. A sender waits up to 500 ms, then reports the player stalled; the next change opens a new player." Today: fix the first sentences, and "debug app builds count audio-thread allocations and log them (step 7)" | move lists are relative to one plan; the stall bound is what's built | 2 (Hold, debug count: 4) |
| Engine shape (step 7) | `Plan`, `DataSet`, `Engine { arena: Map<RuntimeId, Slot>, … }`, six messages | `Renderer { plan: Plan { graph, order_after, cells, out, entry, info }, lanes: Lanes, tempo, arrangement, transport, clock, next_note }`; `Engine { current: Renderer, clock, … }`; `Msg { Activate(Box<Renderer>), SwapLane(generation, slot, LaneSwap), Transport(Cmd), Panic, Hold(bool) }`. "The arena is the boxes in the playing plan's stateful nodes; they move at activation by a move list computed off the audio thread. No RuntimeId reaches the audio thread." | what is built | 2 |
| RENDER-32 | "One `Store` per instance, measured first at 6,176. If that costs too much, one `Store` per partition." | "One `Store` per kernel instance, as G7f measured at p50's count (recorded here). If that costs too much: wasmtime's pooling allocator, or a smaller memory reservation. Never a `Store` shared across plans, which keeps every replaced instance's memory until it drops." | partitions don't exist until 8a, and a shared store leaks | 1 (Today: 2) |
| ID-7 | "A frame … pins SiteIds from step 7." | "A `key()` frame is named by its key in every identity below it; nothing positional above the innermost one is part of an InstancePath." | what pinning means, as built (F6) | 3 |
| ID-13 | "the session MUST give each site a `SiteId(u32)` when it accepts a code revision", rules 1 to 4 | "A site's identity across two revisions is its file and the point of its argument list's `(`, in the reference coordinates of the comparison (WRITE-8). Nothing numbers sites. A Tx is judged in its own edits' coordinates; every other comparison, the engine's included, uses `Edits::between`: a line diff whose hunks a token-level Myers diff refines. A `key()` frame is named by its key. An unmatched or ambiguous site is new, and its state resets." Rule 4 moves to the Deferred row "Keyed iteration helpers and better structural matching" | a counter is session history and would break step 6's two-process hash gate; the engine only compares what plays with what is accepted | 3 |
| ID-14 | as is | add: "Applied when the engine matches instances; a `key()` frame has no occurrence; a plugin with a state path is matched by that path (ID-15)." | built in `continues` (F5) | 3 |
| ID-15 | "RuntimeId = InstancePath / (the site of the `kernel()` or `plugin()` call …)" | "A plugin with a state path (ID-8) has its module key and that path as its RuntimeId, whatever its place. Any other node's RuntimeId is InstancePath / the site of its `kernel()`, `plugin()`, `poly()` or `feedback()` call; segments are `key:<k>` or `<ComponentId>@<file>:<point>#<occurrence>`." | keyed instances must follow their keys (HOST-5 Today); poly and feedback hold state too | 2 (keyed), 3 (segments) |
| WRITE-8 | "each frame a call site given as its byte range … From step 7, SiteIds replace the mapped ranges"; the delta drives `LoadState`, `SetArrangement` | "each frame its ComponentId and its call's point (ID-13)". The delta drives: values → `SetCell`, or an activation when other things change; notes and control lanes → `SwapLane`; audio lanes, assets, states and the arrangement → prepared renders and loads, then an `Activate` of the same structure; structure → `continues`, prepare and `Activate`. Delete the last sentence | as built; folds in step 6's C17 | 3 |
| WRITE-12, WRITE-22 | "carried by the hunks of a line diff" | "carried by `Edits::between` (ID-13)" | one correspondence | 3 |
| HOST-5 | "From step 3 … matched by module, plugin id, ordinal and state hash. From step 7 they persist in the arena by RuntimeId." | "Plugin instances persist across plans like every instance (RENDER-25), by RuntimeId (ID-15) and shape (id, version, channels). A state change is loaded into the instance (RENDER-26b). An instance that asked for a restart is new at the next activation, created from the committed blob. Offline renders in the app share the live instances (RENDER-1); the CLI creates its own." | the ordinal handed one instance's state to another on insertion | 2 (state and export: 4) |
| HOST-6, HOME-4 Today | stale (checked in `understand-contract.md` §4) | rewritten to the code | the contract's own rule | 4 |
| HOME-6 and the `PluginHost::load_state` doc | "skipped when its hash equals the state the instance last saved" | "…equals the state the instance holds: the one it last saved or last loaded, at creation included" | otherwise save, undo, redo skips the redo | 4 |
| DIAG-3 | "from step 7 the notice also compares SiteIds" | "…compares the `(` point of each module's `track()` call (ID-13)", or stays deferred | there are no SiteId numbers | 3 |
| Deferred: "Sending deltas to clients" | "With step 7, or when UI update latency is measured over budget" | "When UI update latency is measured over budget" | a UI concern; it would only enlarge step 7 | 2 |
| Deferred: "Plugin windows, …" | trigger: "The host process exists, and the arena keeps instances alive across plans" | "The host process exists"; and the row adds "an export first commits a pending autosave (RENDER-1)" | the arena exists | 4 |
| Acceptance gate 7 | "6,176 instances" | the count G7f measures (about 6,060 by std's sources) | the gate states what it measured | 1 |
| Stale Today lines | RENDER-6, 9, 12 to 14, 17, 25, 25a, 27, 28, 33, HOST-6, HOME-4 (`understand-contract.md` §4) | rewritten to the code in the slice that changes them | the contract's own rule | 2 to 4 |
| Step 3b design's C7 | pending | withdrawn: nothing adopts | — | 2 |

Not edited, on purpose: PLANE-4 and RENDER-25's "audible under 200 ms after the compile finishes".
G7a measures from the session's request, which is harder (§12.1).

---

## 16. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | One Store per instance at about 6,000 costs too much: about 25 TiB of 4 GiB reservations near the address-space ceiling, slow `Instance::new`, or scattered-store call overhead | G7f measures the real runtime against HEAD first, with a written rule; P3 and P2 fall back in the same code shape; the arena removes today's 2× peak at every activation |
| R2 | The ghost set grows to the whole song (a package change, a master rewire with everything replaced) | 5 ms at 2× DSP, which is today's cost on every edit, now only on rare ones; G7f part 2 includes one |
| R3 | During the window, a ghost reading a kept node gets that node's new output, not its old one | A kept instance has one output. It is still click-free (G6), just not a perfect A/B for 5 ms |
| R4 | A blanked voice compare misses a structural difference, so a pool moves into a graph it doesn't fit | `Voice` `PartialEq` on everything but values, origins, names and capture targets; the audio thread's shape hash; a debug node-count check at entry; `a_changed_shape_starts_from_init` covers voice changes |
| R5 | Myers misaligns repeated tokens (many identical `kernel(` calls, one deleted) and hands a neighbour's state over | Bounded by shape and ID-14 counts; can't click, since consumers' changed inputs fade; keyed plugins aren't exposed (path pass); ID-14 accepts the same exposure for reorders |
| R6 | The export's hold races something: a state load on a borrowed instance, or an activation | While `exporting`, the actor sends no `Activate` and prepares nothing, so no load runs; `Activate` can't apply while held anyway; the guard always reactivates and resumes; probe CI test |
| R7 | Slice 3 changes delta spellings, so an older `Seen(hash)` stops matching | A stale hash is a `Conflict` that rechecks, never a wrong write; noted in slice 3's commit |
| R8 | "A rebuilt kernel module" is reachable only in a runtime test, since packages load once per session | The shape compares content hashes, so any future reload path is safe by construction; G7d tests the rule directly |
| R9 | A plugin glitches on `load_state` while active, or under deactivate and activate | The same calls every host makes for presets and bounces; the "loading" status shows; HOST-3 later isolates crashes; only Surge XT and Airwindows are gated |
| R10 | The slices collide with the data-plane workflow in `crates/server`, `crates/gates`, `core/session` and `web` | Slice 1 is runtime and play only; the rest rebase after it lands; `SwapLane`, `reach` and `delta_between` are left as they are |
| R11 | Until slice 3, an edited track restarts an unkeyed instrument it wraps | That is today's behaviour; keyed plugins already follow their paths in slice 2, as HEAD's `CarryKey` does; slice 2 meets the gate's words (untouched tracks); slice 3 follows directly |
| R12 | The cycle rule is new code with no precedent here | `an_effect_reorder_replaces_the_cycle_and_keeps_the_rest`; random structural edits in `live.rs` |
| R13 | `munmap` storms on the dropper thread stall the audio thread through the VM map lock | Measured in G7f against today's whole-store drop on every edit; batching only if worse |
| R14 | A tempo change re-renders every clip (`ClipKey` includes the whole tempo map), seconds on a long song with warped stems | Today's cost; the old arrangement plays until the renders are ready (RENDER-27). A later cache refinement can key unwarped clips by their start and end seconds |
| R15 | The activation block's own cost (cell compares, wrappers' first copies, ghosts, a second fill per swapped audio lane) | G7b prints it; G7f part 2 bounds the slowest of ten at half the device period; only differing cells are written |
| R16 | The per-block non-finite check costs audio-thread time on every block | About 300 stateful outputs at p50, under 20 µs (*estimate*); it shows in G7f part 2's p99 against HEAD |
| R17 | A reset at entry (a poisoned box) is slow or traps | The same reset `Panic` makes, for the poisoned boxes only; a trapping reset leaves the plan broken (silent), as a trap would, and the next edit tries again |

---

## 17. Attack response

Twenty-one problems were raised against revision 1. Several were raised twice; they are answered
together.

| # | Problem | Verdict | Why | Where it changed |
|---|---|---|---|---|
| A1 | `Jump::Enter` hides a Stop, Seek, Start or block-line wrap on the activation's line, so notes stick and moved audio players click | **Accept** | Confirmed in `Transport::advance` and `NoteScheduler::run` (`run.jump != Enter`). With moved schedulers and players, nothing else ends those notes or declicks. A real jump does exactly what is needed, and for a fresh scheduler it drains nothing and seeks, as `Enter` did | §8.1 (the new order), §7.2 step 2, §2 P5, §13 (`a_jump_on_the_activation_line_ends_its_notes`, `a_jump_on_an_entry_line_wins`, `live.rs` jumps on activation lines), §15 RENDER-26a; slice 1 |
| A2 | A kept plugin never receives an edited plain-number parameter | **Accept, with the smaller fix** | Confirmed: `plugin_events` sends `Input::Const` only when `last.is_none()`, and the checker folds plain numbers into `Input::Const`. Making the `Const` arm send when `*last != Some(v)`, as the `Port` arm does, is enough: with it, a copied last value can never hide an edit, so the "copy only when the input is unchanged" rule isn't needed. With-plugins' own gate would have caught it (Airwindows' plain-number `dry_wet` edit) | §10.1, §5.1, §13 (`a_changed_constant_parameter_reaches_a_kept_plugin`, `a_changed_constant_is_sent`); slice 1 |
| A3 | Keyed plugins are matched by call site before state path: a key swap or rename keeps the wrong patch, and from slice 4 a save overwrites another key's preset | **Accept** (with A11) | `plugin.state-collision` makes `(module, path)` unique, and HEAD's `CarryKey` already keys by it. The path pass runs first, from slice 2. Putting the path, or a "has a blob" bit, in `Shape::Plugin` is **rejected**: the path pass makes paths equal for every pair it makes, and a blob bit would restart an instance at its own first save. Slice 4 compares blobs per node instead, so a pairing across paths loads the right blob even when no file changed | §3.2 (path pass), §3.3 (F7 deleted), §5.2, §6.2, §10.2, §13, §15 ID-15 |
| A4 | An audio `SwapLane` crossfades identical material with equal-power gains: +3 dB blip, and no-op swaps aren't bit-exact | **Accept** (with A10). The proposed prepare-side skip of unchanged clip lists is **rejected** | Equal-power is right only when the two sides are different material. Swaps now mix the two sides with `xfade`, linear and exact where equal. The skip would never fire: on a tempo change every `ClipKey` changes, on a meter change no audio lane swaps, and a lane the delta lists has a changed clip; the equality branch already keeps unchanged clips exact | §8.2, §7.4, §12.3, §13 (`an_audio_swap_of_unchanged_clips_changes_no_bit`, `a_clip_gain_edit_stays_between_its_two_levels`) |
| A5 | A tempo change re-renders only warped clips, but unwarped clip positions and lengths depend on the tempo map too | **Accept** | Confirmed in `AudioLane::build` and `render_clip`. On a tempo change every audio lane gets new renders through the cache; the old renders continue at their own position (`shift`) through the crossfade | §8.4, §6.1, §2 P12, §13 (the tempo test gains an unwarped clip), §15 RENDER-27, R14 |
| A6 | A smooth voice-cell ramp in flight at activation is never ended in the moved pool: a 375 Hz sawtooth | **Accept** | Confirmed: `write_const` leaves a `Ramp` buffer with version `COPY`, and the new plan's `ramping` is empty. `take_over` ends the old plan's ramps before any move, as its next block line would; no allocation | §7.2 step 0, §9.1, §2 P4, §13 |
| A7 | The plan-wide taint flag wipes every tail after one stray sample, misses a NaN that doesn't reach the output, and races prepare | **Accept the problem, with another fix** | Per-box flags, as proposed, but kept in the box and read on the audio thread at entry: set by a non-finite output (cleared by the next finite block) or a trap, and answered by resetting that box in place as `Panic` would. No shared atomics, nothing for prepare to read, no race, no refusal path, and a hidden NaN is caught at its own box | §7.7, §7.2 step 1, §5.1, §2 P9, §13, §15 RENDER-25 and RENDER-10; `tainted` and `wipe` deleted from §4, §5.3, §6 |
| A8 | `AudioPlayer.prev` keeps the previous renders until the lane's next swap, doubling each edited stem's memory | **Accept** | `prev` is read only for one window, and the window's end takes it into the leftovers | §8.2, §7.3, §7.6, §12.7 (audio edits on `examples/resample`, renders freed) |
| A9 | Moves and the window's end are assignments that free heap memory on the audio thread | **Accept** | Every box, lane and order move is a `mem::swap`; `LaneSlot::Control` becomes an `Option`; verify checks that each new lane slot is empty. `no_alloc.rs`'s trap already counts deallocations, and it now covers the window's end and a player's `prev` | §4.2, §7.2, §7.3, §13 |
| A10 | An audio-lane swap uses the jump's equal-power crossfade on correlated material (high) | **Accept**, merged with A4 | As proposed: two fills over the same runs, one through a copy of the declick state, mixed by `xfade`. `Tap.src` and the re-pointing of taps are never built | §8.2, §14 "Never built" |
| A11 | Keyed plugins match by position first; slice 2 regresses HEAD's `CarryKey` | **Accept** | One rule: a keyed plugin's RuntimeId is its module and state path, in slice 2, so HOST-5's guarantee holds in every slice. F7 is deleted. A keyed plugin with no path partner goes on to the name pass, so adding a key keeps its instance | §3.1, §3.2, §3.3, §13 (`keyed_plugins_follow_their_paths`), §15 ID-15 and HOST-5, R11 |
| A12 | Audio-lane edits get two carriers, `SwapLane::Audio` and the Activate's lane moves | **Accept** | RENDER-27 already routes an audio change that needs a render with the arrangement. Audio, asset and state edits become identity `Activate`s; `SwapLane`, its readiness and `reach` stay exactly as HEAD has them. A plan that swaps audio has a window, so "no window running" covers the player's crossfade | §7.1, §8.3, §8.5, §6.1, §15 (RENDER-26 edit dropped, planes table), §14 |
| A13 | Taint is sticky and over-broad, and `wipe` duplicates `Way::Replace` | **Accept the problem; fixed as A7** | "Broken now", as proposed, but per box rather than per plan. `Way::Replace` isn't needed for it: a reset in place heals plugins too (`host.reset`), and keeps every other tail, where a replace restarts the whole engine | §7.7 |
| A14 | Slice 4 can't land green: `new_state` and the engine rig still expect a changed state to be a new instance | **Accept, with a simpler fix** | No separate states step: the loads are the last step of `prepare`, which the server and the rig both call, and a state-only revision is an identity `Activate`. `new_state` is rewritten in slice 4 as proposed | §10.2, §8.3, §12.8, §13, §18 slice 4 |
| A15 | Export save-and-restore and restart from a fresh save treat in-memory state as different from the committed blob | **Accept** | Until windows ship the instance holds the committed blob plus code-owned parameters; restoring a save is what HOME-6 forbids and costs a patch load per instance. Restart is from the committed blob and moves to slice 2. RENDER-1's edit says the instance is left holding the committed blob, and that an export first commits a pending autosave once windows ship | §10.5, §10.3, §12.5, §15 RENDER-1, Q7 |
| A16 | The RENDER-25 edit moves the 200 ms budget's start to after check and verify | **Accept** | The contract stays; G7a measures from `requested_ns`, which is harder, and prints `compiled_ns` too | §12.1, §15 (RENDER-25 budget unchanged; "Not edited, on purpose") |
| A17 | G7b's 1.25× activation-callback bound is the wrong measure and makes a bit-exactness gate flaky | **Accept** | G7b prints it; the budget is stated once, in G7f part 2's recorded run: the slowest of ten activation callbacks under half the device period | §12.2, §11.3, R15 |
| A18 | `continues` joins on every settle, on a data plane already over budget | **Accept** | `reach` keeps `delta_between`; only the code-changing `Activate` branch calls `continues`, which recomputes the comparison and joins. Identity is used when the code is the same | §3.2, §6.11, §8.3 |
| A19 | `Hold` breaks RENDER-6a without a contract edit | **Accept** | RENDER-6a gains the export exception in slice 4; the hold test cites it | §10.5, §13, §15 RENDER-6a |
| A20 | §9.3's conditional deletion of the value fast path is outside step 7's scope | **Accept** | Kept as built; the G7a value-edit variant and RENDER-22's "After §9.3" wording are dropped | §9.3, §12.1, §15 RENDER-22, §14 "Kept on purpose"; revision 1's Q10 and its fast-path risk removed |
| A21 | G7f picks the layout from a re-implementation and measures layouts that are only fallbacks | **Accept** | The same harness file runs on HEAD (S) and slice 1 (P1), using only `Renderer::new`, `render_block` and `Drop`; P2 and P3 only if P1 fails; T and the leak control dropped; the rule also judges part 2's real numbers | §11.2, §11.3, §12.6 |

---

## 18. Build slices

**Coordination.** The data-plane workflow is editing `core/session`, `crates/server`, `crates/gates`
and `web` now. Slice 1's runtime and play work touches none of them and can start at once. G7f's
harness is a new file in `crates/gates` plus one registration line; its S run happens from a worktree
at HEAD (sharing the main target dir, removed after), and the file lands once that workflow does.
Slices 2 to 4 rebase on it. Slice 3 can run beside slice 2's second half; the two meet only in
`continues.rs`. Step 7 leaves `SwapLane`, `reach`, `delta_between` and the value fast path as that
workflow writes them.

### Slice 1: movable instances, and the Store measured (about 1.5 weeks)

Nothing audible changes, except two fixes that only matter once instances move.

**What lands, in order:**

1. **`KernelInst` with its own `Store`.** `KernelNode` splits into a wrapper (`inputs`, `seen`) and
   `inst: Option<Box<KernelInst>>`. The `store` argument goes from every `Graph` function. Add
   `LIVE_KERNELS`.
2. **`PolyNode`** splits into a wrapper and `pool: Option<Box<VoicePool>>`. `Exec::Capture(PortRef)`
   becomes `Exec::Capture(u16)` into `PolyNode.captures`, and the voice `Graph::process` takes
   `(outer, captures)`.
3. **`AudioPlayer` in `LaneSlot::Audio`.** It is `AudioNode`'s state (as a `Copy` `Declick`) plus the
   renders, filled once per lane per block. `Exec::Audio { lane }` copies it. No swap yet.
4. **Engine-wide note ids.** `Renderer.next_note` is passed to `scheduler.run`, and `enter_from` copies
   it. Delete `NoteScheduler.next_id`, `next_id()`, `raise_next_id`, `PluginInstance.next_id`,
   `held_cap` and `hold`; `carry_from` stops raising ids.
5. **Two one-line fixes:** `Transport::advance` lets Start, Stop, Seek and Wrap win over `Enter`
   (§8.1), and `plugin_events`' `Const` arm sends when the value differs from the last one sent (§10.1).
6. **`Plan` and `Lanes` inside `Renderer`** (§4.2), with `plan.rs` holding `Shape`, `hash64`,
   `PlanInfo` and `PackageSet::module_sha`, and every `LaneSlot` variant an `Option`.
   `prepare(.., Basis::Fresh)` is the only basis. `build_live` stays as a wrapper that still takes the
   `InstanceSource`.
7. **G7f, part 1** (`crates/gates/src/store.rs`): S recorded at HEAD before commit 1, P1 here. The
   layout is decided by §11.3's rule. If P1 fails, the engine configuration changes in
   `packages::engine()` now, before anything depends on it.

**Tests:** `one_player_per_lane_gives_the_old_bits`; `a_jump_on_an_entry_line_wins`;
`a_changed_constant_is_sent`; `commands_land_on_the_block_line_as_one_jump`'s last assertion updated
(§13).

**Stays green because:**

- every other existing test passes unchanged: `bit_identity`, `no_alloc`, `live.rs`, `carry.rs`,
  `cell.rs`, `tests.rs` and the gates. The transport fix changes only a line where an activation and a
  jump coincide, which today's whole-output crossfade covers; the `Const` fix sends nothing new
  offline;
- offline renders are byte-identical (`renders.json`);
- `prewarming_keeps_kernel_memory_flat` and `kernel_memory_bytes` hold with per-instance stores.

**Proves:** G7f ("measured, and the store layout chosen from it"), with acceptance gate 7's count;
G7c still holds.

Size *estimate*: +700 / −400 lines in `core/runtime`, about 40 in `core/play` tests, about 200 for
the G7f harness.

### Slice 2: the arena (about 2.5 weeks)

**What lands:**

- **`core/runtime/src/plan.rs`** (about 800 lines): `Basis::Keep`, the matching, edges, ghosts, ghost
  lanes, fade nodes and Kahn order with the cycle rule; `Entry`, `Stats`, `take_over` (verify, then
  move by swap, with the ramp end and the reset of poisoned boxes), `end_fade`, `take_from` per kind.
- **`core/runtime/src/fade.rs`:** `xfade`, and `Exec::Fade`.
- **`lib.rs`:** the per-box `poisoned` flag and its live check in `Graph::process`; `Cells::end_ramps`;
  cell carry.
- **`audio.rs`:** `AudioPlayer::swap` (two fills, `xfade`, `shift`), `prev` handed to the leftovers;
  the `Jump::Enter` arm becomes a no-op; `LaneSwap::Audio` for lane moves only; a `Cache` test hook
  for weak render handles.
- **`engine.rs`:** `try_apply` calls `take_over`; delete `Fade`, `fading`, the two-renderer mix in
  `render`, the coalescing branch, `heard[1]`, and the fading arms of `apply_now` and `resets()`; add
  `end_fade` at block lines, `Retired::Nodes`, `refused_activations`, `leaked`, and `Applied.refused`.
- **`sched.rs`:** delete `adopt`, `adopted`, `released` and `release_all`; add `take_state`.
- **`plugins.rs`:** delete `CarryKey`, `Carry`, `InstanceSource`, `Fresh`, `carry_from` and `lend`;
  `PluginInstance` keeps `host`, `h`, `rate` and its `Drop`, and gains `shape`; `PluginNode.lent`,
  `PluginView.lent`, `render_block_lent` and `render_block`'s `release_all` go; so do the runtime's
  `in_voice` plugin branch and `Builder.{ordinals, source}`.
- **`core/session`:** `tx/continues.rs` (§3.2: the path pass and the name pass) and the exposed `ids`
  in `delta.rs`, about 100 lines. `delta_between` is unchanged.
- **`crates/server/src/graph/actor.rs`:** `Plays.info: Arc<PlanInfo>` replaces `slots`, `caps`,
  `cell_slots` and `cells`; `Open.carry` goes and `Open.restarts: HashSet<Handle>` arrives, with
  restart from the committed blob (§10.3); `reach` is unchanged, and its `None` branch prepares with
  `identity` when `code_rev` is unchanged and `continues` otherwise (§8.3); the `Way` variants lose
  their `Carry`.
- **`crates/gates`:** the rig on `Basis`, `--no-arena`, `Probe` stats, `output_of`, the `tails` gate
  (G7b), `--at` and `--start` (G7a), and G7f part 2.
- **Contract edits that land with it:** Planes table (audio, assets, arrangement, code rows),
  RENDER-10, 16, 22 Today, 23, 25, 25a, 26 Today, 26a, 27, 28, 32 Today, the Engine shape, ID-15's
  keyed half, HOST-5's first half, the Deferred row on deltas to clients, and C7's withdrawal.

**Tests:** `keep.rs` (all of §13's RENDER-25, 25a, 26a, 27, 23 and 22a, HOST-5, 28 and Panic
entries); G7c's `a_no_op_activation_changes_no_bit` and its variants; G7d's test and controls; the
extended `no_alloc.rs`; the churn test with audio edits; `live.rs` with structural edits and jumps on
activation lines; the session's `continues` tests (slice 2 set, `keyed_plugins_follow_their_paths`
included); the server tests marked slice 2 in §13.

**Stays green because:**

- every step 3 gate passes at p16 and p50, and with-plugins passes with "kept" in place of "carried":
  `new_state` still sees a new Surge, since the shape still holds the state hash, and "the new value
  reached the carried instance" (Airwindows' plain-number `dry_wet`, 0.35 → 0.9) passes because of
  slice 1's `Const` fix;
- keyed plugins follow their paths, so nothing HEAD's `CarryKey` kept is lost;
- `SwapLane`, `reach` and the value fast path are untouched, so the data-plane gates (G8 `previews`,
  G9 `drag`, G11) and the lease tests pass as they are;
- `a_failed_build_then_a_lane_edit_rebuilds` passes on the new `Plays`.

**Proves:** G7a (heard within 200 ms of the request at p50 from 3:00), G7b's untouched tracks (bit for
bit, with the negative controls failing), G7c (no-op activations and rebuilt lane data change no bit),
G7d (a rebuilt module starts from init; identical bytes keep), and G7f part 2 (the arena's p99 and the
activation block's time).

Size *estimate*: +1,400 / −900 lines.

### Slice 3: names that survive edits (about 1 week)

**What lands:** F1, F2, F3, F5 and F6 (§3.3), in `core/session/src/text.rs`,
`core/script/src/sites.rs`, `core/session/src/tx/delta.rs` and `continues.rs`. G7b's wrapped-instrument
variant, through the Tx and as a file save. Optional: the editor's text carry onto `Edits::between`,
and the deletion of `m2_ir::control_id`. **Contract edits:** ID-7, ID-13, ID-14, ID-15's segments,
WRITE-8, WRITE-12, WRITE-22 and DIAG-3.

**Tests:** the slice 3 `continues` tests, the `text.rs` tests, the `delta_tests.rs` set and
`every_call_has_an_open_paren` (§13).

**Stays green because:**

- step 6's tests pass, including `delta_names_are_unique_on_every_fixture` with its new spelling and
  the two-process hash test;
- the expectation tests are unchanged, since they judge by spans;
- keyed plugins don't depend on names, so with-plugins is unaffected.

**Proves:** G7b's wrapped instrument (the edited track keeps its instrument, and the whole mix is bit
for bit through the window, with slice 2's names as the failing control), and G7a's "at most 3 fresh
instances per edit".

Size *estimate*: +280 / −120 lines.

### Slice 4: plugins persist, and export (about 1 week)

**What lands:**

- **State loads into kept instances:** `Shape::Plugin` loses `state`; `PluginAt.blob`; the per-node
  blob compare, the removed-blob rule and the loads as prepare's last step (§10.2);
  `PluginInstance.reloaded`; the latency warning; the `holds` host fix.
- **Export:** `Msg::Hold`, `Status.held`, `Player::hold` and `held`, `Basis::Borrow` and `reset_first`,
  the export thread's protocol and its guard, `Open.exporting`, and `ExportDone`. No save, no restore.
- **Status:** "loading <plugin>" and "paused for export" in the engine view and the web's status area.
- **A debug-build allocation counter in `apps/music2-desktop`,** which RENDER-28 asks for: the counting
  allocator from `no_alloc.rs`, made a module of `m2-play`. It logs once a second while its count is
  nonzero, and never panics in the callback.
- **Gates:** G7e (the server gate and the probe CI test); with-plugins' `new_state` rewritten (§12.8).
- **Contract edits:** RENDER-1, RENDER-6a, RENDER-26b, RENDER-28's `Hold`, HOST-5's second half,
  HOST-6 Today, HOME-4 Today, HOME-6, the `PluginHost::load_state` doc, and the Deferred plugin-windows
  row.

**Tests:** `a_reload_after_another_load_is_never_skipped`, `a_load_of_the_held_state_is_skipped`,
`a_state_load_resends_code_owned_parameters`; `a_state_change_loads_into_the_running_instance`;
`swapping_two_plugin_keys_moves_the_instances` with distinct blobs; the G7e probe test;
`settles_during_an_export_wait_then_apply_once`; `lane_previews_play_during_an_export`;
`loading_is_published_while_a_plugin_is_created`; `a_hold_freezes_and_resumes_without_a_step`;
`no_alloc.rs`'s hold case.

**Stays green because:** the rewritten `new_state` passes in both rigs, since both call `prepare`;
with-plugins' G5 and G7 pass; the `core/plugins` tests and `main_thread.rs` pass with the probe; a pure
song's export runs beside playback as before.

**Proves:** G7e (the export pauses playback, reuses every instance, and playback resumes where it was
with each instance's state bytes unchanged).

Size *estimate*: +450 / −100 lines.

**In all,** about 6 weeks of work, about 5.5 elapsed with slice 3 overlapping, within the order of
work's 4 to 6.

---

## 19. Owner questions

Only the decisions this design can't make on its own. Each has the design's answer.

| # | Question | This design's answer |
|---|---|---|
| Q1 | Is a SiteId a point or a counter (ID-13)? | A point: the file and the `(` of the call, compared between what plays and what is accepted. A counter is session history and breaks step 6's two-process hash gate. |
| Q2 | Is a `poly` kept or replaced whole (RENDER-16)? | Whole. An edit inside an instrument's voice graph restarts that instrument, as today; everything else keeps. A rewired instrument is kept and its notes released. |
| Q3 | Where do structural fades fall (RENDER-25a's "at least at every module output")? | On changed edges into kept nodes and on the output, linear and exact when equal; replaced nodes ring out for 5 ms as ghosts. Fading at module outputs would need kept instances processed twice. |
| Q4 | What happens on a reorder of kept effects? | The reordered effects restart (the cycle rule); nothing else does. |
| Q5 | A plain number (not a declared control) edited inside an instrument body? | It is part of the voice shape, so it restarts the instrument. Declared controls are cells and don't. |
| Q6 | What does "pause" mean during an export with plugins? | A tape pause: a 5 ms fade out, frozen, then a 5 ms fade back in where it was; kernel tails resume, plugin voices were reset. Transport pressed meanwhile applies at the resume. The alternative is a stop: every note ended, and play pressed again. |
| Q7 | After an export, is "the state they had" the committed blob (RENDER-1)? | Yes: each instance is left holding the committed blob, with its code-owned parameters sent again, and nothing is saved or restored. That is the same state until plugin windows ship; the windows step must commit a pending autosave before an export. |
| Q8 | No `LoadState` or `SetArrangement` messages, and activations never coalesce (RENDER-26b, RENDER-27, RENDER-28)? | Yes: a state load is a main-thread call made in prepare, an arrangement change is an `Activate` of the same structure, and each activation names the plan it was prepared against. Step 3b's C7 is withdrawn. |
| Q9 | std's mixer under ID-14 | Adding a channel resets every strip inside the mixer's `map`. That is inaudible today (`std:gain`'s smoother starts at its target; pan holds no state), but a stateful effect inside such a map would lose its tail. Proposed std follow-up, outside step 7: key each strip by its channel key. |
