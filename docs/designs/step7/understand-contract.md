# Step 7, the engine: what the contract asks, what earlier steps left, how the gate is measured

Read against `docs/architecture.md` at HEAD `cbf5eb9`. Read-only: no cargo was run. Files that the
data-plane workflow is editing right now (`crates/server/src/graph.rs`, `graph/actor.rs`,
`crates/gates/src/server.rs`) were read at HEAD with `git show`, so the line references below are
HEAD's.

Step 7's line in the order of work:

> `Plan` split from `DataSet`; SiteId matching, InstancePath and RuntimeId. The arena, with prepare
> and activate, keeping instances whose RuntimeId and shape (wasm hash included) match. Arrangement
> switches prepared off the audio thread; persistent CLAP instances behind `PluginHost`, with
> `LoadState`, and offline renders that reuse them.

Gate:

> In p50, a structural edit at 3:00 is audible within 200 ms after the compile, and the tails of
> untouched tracks are intact. Live playback with no edits is bit-identical to offline. A kernel
> module rebuilt with the same kernel id but a new hash starts from init. An export of
> `examples/with-plugins` pauses playback, reuses its instances, and playback resumes with their state
> intact. The per-instance `Store` cost at 6,176 instances is measured, and the store layout is chosen
> from it.

Sections:

1. The engine as it is at HEAD (the data flow and the audio-thread handoff step 7 changes)
2. Every rule that speaks to step 7, with its Today checked against the code
3. What earlier steps deferred to step 7
4. Today lines that are stale (do not design against them)
5. The gate, item by item, and how to measure each with `crates/gates`
6. Rules whose complexity looks unnecessary, with proposed contract edits
7. Guarantees that must not move, and how each is already tested

---

## 1. The engine at HEAD

What plays is one `Renderer` (`core/runtime/src/lib.rs:1237`). It owns everything: one wasmtime
`Store<StoreLimits>` for every kernel instance it has (`build_live`, lib.rs:1284-1350; each kernel is
`Instance::new(&mut store, module)`, lib.rs:756), the graph (`Graph { exec, order, bufs, versions }`,
lib.rs:553), its cells (`Cells`, `core/runtime/src/cells.rs`), its lane slots (`LaneSlot::{Notes(NoteScheduler),
Control, Audio}`), the tempo map and arrangement, and the transport. Stateful node kinds in `Exec`
(lib.rs:529): `Kernel`, `Plugin`, `Poly` (voice slots, each a `Graph` of kernels, plus the allocator),
`Feedback { ring }`, `Audio(AudioNode)` (jump taps), and the note schedulers in `LaneSlot::Notes`.

The path of a change, server to speaker:

1. The session's actor (`crates/server/src/graph/actor.rs`, HEAD) settles after every accepted
   revision. `Open::way` (actor.rs:409) compares what plays (`Plays.revision`, an `Arc<Revision>`) with
   the accepted revision through `m2_session::tx::delta_between`, in `Open::reach` (actor.rs:376):
   - values only, and the two `Flat`s number their cells alike (`same_layout`, `value_writes`,
     actor.rs:151-205): cell writes into what plays (`Player::set_cells`);
   - notes and control lanes only, each fitting its scheduler: `SwapLane` per lane;
   - anything else (structure, a state blob, an asset, the arrangement, an audio lane, a rate change):
     a whole new `Renderer`, built on the actor's thread by `Renderer::build_live` with plugin instances
     taken from a `Carry` of the renderer last sent (`core/runtime/src/plugins.rs`), every kernel from
     `init`.
2. `Player::activate` (`core/play/src/lib.rs:686`) puts `Msg::Activate { renderer }` on a bounded
   crossbeam queue (1024). Senders `try_send` and retry for up to 500 ms, then report `Stalled`.
3. The device callback (`Callback`, play/lib.rs:207) runs the `Live` (engine plus resampler). The
   engine (`core/runtime/src/engine.rs`) reads messages at each 128-frame block line. Transport and
   panic apply at once; structural messages apply in order (`head`, `blocked`), and consecutive
   activations coalesce (engine.rs:448).
4. `Activate` (engine.rs:351-366): the new renderer takes the old one's transport
   (`Renderer::enter_from`, which keeps the beat across a tempo or rate change via `Transport::retime`),
   takes over the shared plugin instances with the notes sounding into them (`plugins::carry_from`),
   and the old renderer becomes `Fade`: both render for 5 ms, the old one copying the output of lent
   plugin nodes instead of processing them (`render_block_lent`, `plugins::lend`), and the whole
   output crossfades linearly (engine.rs:495-515).
5. The old renderer is retired: `retire` callback to the player's `Gone` channel (64), else the
   engine's 16-slot stash, else the player's 8-slot `gone`; a dropper thread drops them every 5 ms
   (`drop_retired`, play/lib.rs:505). Plugin instances are destroyed on the plugin main thread when the
   last `Arc<PluginInstance>` goes (`PluginInstance::drop`).
6. Cells never use the queue: `CellSlots` per renderer, atomics only on the audio thread, applied at the
   renderer's next block line (`Cells::apply`).

Offline: `m2 render`, Export Audio (`crates/server/src/export.rs`, through `Actor::render`, actor.rs:889)
and range renders build their own `Renderer::new` / `Renderer::starting_at`
(`crates/editor/src/render.rs:133`, 161), so every plugin is a second, fresh instance.

So at HEAD: plugin instances survive a structural edit (step 3b), kernels never do, and every
structural, arrangement, audio-lane, asset or state change cuts every kernel tail in the song behind a
5 ms whole-output crossfade.

---

## 2. Rules that speak to step 7

"Today (checked)" is what the code does at `cbf5eb9`, not what the contract's Today line says. Where they
differ, section 4 lists it.

### 2.1 Planes (the budgets step 7 must meet)

| Rule | What it asks of step 7 | Today (checked) |
|---|---|---|
| Planes table, **Prepared data** row | audio lanes, assets, plugin state: no QuickJS; clip renders and state loads prepared off the audio thread; `SwapLane`, `LoadState`; "the old data plays until the switch" | a new renderer for any of these (actor.rs:383, 404); no `LoadState` |
| Planes table, **Arrangement** row | timing tables and clip renders prepared off the audio thread; `SetArrangement`; no glitch | a new renderer, which keeps the beat (`enter_from`) but restarts every kernel |
| Planes table, **Code** row | "compile, check, verify, new plan; `Activate`": audible under 200 ms after the compile, plugin loads not counted (RENDER-25) | a whole renderer built per structural edit |
| PLANE-2 | a mixed change takes the slowest plane (Code, Arrangement, prepared data, Data) | matches (`take::Plane`) |

### 2.2 Render

| Rule | What it asks of step 7 | Today (checked) |
|---|---|---|
| **RENDER-1** | Offline renders exact from sample 0, every instance from init. In the app, an offline render of a song with hosted plugins pauses live playback and reuses the live instances through `PluginHost`: `deactivate`, `reset`, `load_state` of the committed blob, `set_render_mode(Offline)`, `activate`; afterwards back to realtime with the state they had. A pure song's export runs beside playback; the CLI creates its own. | Export creates a second set of instances on its own thread (`export.rs`, `Render::new` → `Renderer::new` → `Fresh`). The host has every call needed (`core/plugins/src/host.rs`: `deactivate`, `reset`, `load_state`, `set_render_mode`, `activate`, `save_state`). |
| **RENDER-10** | Reset only on panic, optional stop, start of an offline or range render, and per voice. "An instance that a plan activation doesn't match starts from init; that is a new instance, not a reset of kept state." | Every structural edit starts every kernel from init (new renderer). |
| **RENDER-16** | "A voice's RuntimeId is its allocator slot." | `PolyNode.voices: Vec<VoiceSlot>` (lib.rs:243); nothing keyed. |
| **RENDER-17** | Latency per instance, measured after state load. "Live applies a new latency only at a plan activation, through `deactivate` and `activate` …, with the crossfade of RENDER-25a." | Latency is measured at `activate` after `create` loaded the state (`host.rs` doc of `activate`). Nonzero latency is refused at check time (`core/ir/src/check.rs:680`, RENDER-21), plugins included (their latency is described at default state, `packages.rs:379-386`). |
| **RENDER-22** | A cell is written into kernel input memory "once when its plan activates", and again only on `SetCell`. | Matches per renderer (`cells.rs`). |
| **RENDER-23** | `CellId` from the active plan; never outlives its plan. Today line: "cells live and die with their renderer until the arena keeps them across plans (step 7)". | `CellId` is an index in a renderer's table; leases remap through `PlaysView::target_of` (`crates/server/src/graph/lease.rs`). |
| **RENDER-25** | Plans prepared off the audio thread, activated at a block boundary. Activation keeps every instance whose RuntimeId and shape match. Shape = kernel id, content hash of the kernel's wasm module, channel counts, state ABI, settings; for a plugin, plugin id and version. Others start from init. Replaced objects retired off the audio thread. New plugin instances created, loaded and activated on the plugin main thread while the old plan plays; the UI shows "loading <plugin>". | Whole renderer swapped; plugins carried (3b), kernels not. Retiring never drops on the audio thread (engine stash, player `gone`, leak as last resort with a `debug_assert`). No "loading <plugin>" text found in `web/src` or `crates/server/src`. |
| **RENDER-25a** | An activation that replaces a subgraph crossfades over 5 ms: "old and new instances run together for one fade window, and the fade is applied at least at every module output whose subgraph changed. An added or removed effect fades between dry and processed the same way." "Step 3, which still swaps whole renderers, crossfades the whole output." | Whole-output linear 5 ms crossfade (engine.rs `Fade`). |
| **RENDER-26** | `SwapLane` replaces one lane's data at a block boundary without touching a kernel; from step 6 the delta decides. | Matches (`Open::reach`); addressed by renderer `generation` and lane slot index. |
| **RENDER-26a** | Each scheduler owns its sounding notes. "An activation that replaces a note consumer first sends an Off for every note sounding into it." "`SetArrangement` recomputes the frame of every pending Off from its beat." | Built in 3a/3b. A carried plugin's notes go on through `NoteScheduler::adopt` (step 3b's clarification C7, still awaiting the owner). Sounding notes keep `end_beat` (`sched.rs`, `Sounding`), so a recompute is possible. |
| **RENDER-26b** | A state change from outside the instance reaches it as `LoadState`, loaded on the plugin main thread, skipped when its hash equals the instance's last save (HOME-6), latency measured again; "if the latency changed, a plan-only activation re-plans the compensating delays, with no compile". A changed asset takes RENDER-27's path. | A changed blob gives a new instance (`CarryKey.state`, plugins.rs:40-42: "no `LoadState` until step 7"). `InProcess::load_state` exists and runs on the main thread, skipping a blob equal to the last save (`core/plugins/src/inprocess.rs:275-282`). |
| **RENDER-27** | Arrangement change: prepare timing tables and every affected clip render off the audio thread, switch together at one block boundary, keep the beat. An audio-lane change that needs a render takes the same path. | A new renderer: clip renders come from the shared `audio::Cache` (built on the actor thread), the beat is kept (`enter_from`), every kernel restarts. |
| **RENDER-27a** | A tempo drag is a lease; warped audio may lag during the drag. | Not built (step 8b's tempo bar). |
| **RENDER-28** | Audio thread never allocates, locks, does I/O or waits on a compile. Structural messages (`Activate`, `SwapLane`, `SetArrangement`, `LoadState`, `Transport`) over a bounded queue, never dropped; when full, the sender waits or "coalesces to the newest plan". Debug builds and CI trap allocation. | Matches for what exists; `core/play/tests/no_alloc.rs` runs two probe plugins through an activation that carries both, under the trap, with cell writes from another thread. |
| **Engine shape (step 7)** | `Plan { nodes, order, buffers, cells, latency }` (structure only, from `Flat`); `DataSet { arrangement, lanes }`; `Engine { arena: Map<RuntimeId, Slot>, plan, data, cells, engine_clock, transport }`; `Msg { Activate(Plan, Slots), SwapLane(LaneAddr, Arc<LaneData>), SetArrangement(Arc<Arrangement>), LoadState(RuntimeId, Arc<Blob>), Transport(…), Live(Event) }`. | `DataSet` exists, in `m2_song` (`core/song/src/data.rs:64`): arrangement, tempo, lanes, verified state paths and hashes, assets, dir. No `Plan`; `Flat` is turned straight into a `Renderer`. Engine `Msg` is `Activate { renderer }`, `SwapLane { generation, slot, data }`, `Transport(Cmd)`, `Panic`. |
| **RENDER-32** | "One wasmtime `Store` per instance, measured first at 6,176 instances. If that costs too much, one `Store` per partition." | One `Store` per renderer (lib.rs:1293-1296). Each `PackageSet` has its own wasmtime `Engine` (`packages.rs:93-102`). std is one wasm module for all its kernels (`packages.rs:134-141`). |
| **RENDER-33** | Live from transport 0, fresh engine, no edits, any buffer size: same `f32` bits as offline; CI on every pure fixture. | `core/play/tests/bit_identity.rs`: every pure fixture (whole song), p16 (30 s in CI; all 4:02 `--ignored`), and a no-op `SwapLane` changes no bit. |

### 2.3 Identity

| Rule | What it asks of step 7 | Today (checked) |
|---|---|---|
| Identity table | Derived: SiteId, InstancePath, ControlId, RuntimeId, CellId; memory only. | — |
| **ID-7** | A `key()` frame "pins SiteIds from step 7". | `key()` frames compose state paths. The step 6 delta already names a node inside a `key()` frame by `key:<k>`, not by position (`Side::chain`, `core/session/src/tx/delta.rs:250`). |
| **ID-8** | A plugin without a key "has an in-memory RuntimeId". | Carried by `CarryKey { module, plugin, version, state_path, ordinal, state }`. |
| **ID-12** | Build-local numbers (site ids, ordinals, node numbers, `Origin`, control ids) never cross revisions; resolve again from source position. | Holds; sites are numbered per build (`core/script/src/lib.rs`). |
| **ID-13** | "From step 7, the session MUST give each site a `SiteId(u32)` when it accepts a code revision, by matching against the previous code revision in this order: the Tx's own correspondence; `key()`; carrying through a token-level Myers diff; a unique structural match on callee, scope and argument shape, ignoring bindings and literal values. An ambiguous match gets a fresh id, and its state resets." | Not built. Step 6's delta names do items 1 to 3 in their own way: a Tx's own edits, `key:<k>` frames, and the carry of WRITE-12 (a line diff trimmed to bytes, `core/session/src/text.rs`), as points in reference coordinates. Item 4 has nothing. |
| **ID-14** | Repeated unkeyed sites in one parent (loops, `map`): state kept by index only while the occurrence count and surrounding structure are unchanged; if the count changes, every occurrence resets. | The delta names occurrences `#k` and keeps `#0..#n-1` when a loop grows to `n+1`: the engine matcher must add ID-14's "count changed, keep none" filter. |
| **ID-15** | Frames `{ site, componentId, occurrence, key? }`; InstancePath = module key / frame chain; ControlId = InstancePath + slot; RuntimeId = InstancePath / (site of the `kernel()` or `plugin()` call in the innermost frame, its occurrence), matched by ID-13 and ID-14; a voice's RuntimeId is its allocator slot; CellId = index in the active plan. | `Module.frames`, `Node.frame`, `Origin.control`, `m2_ir::control_id` exist (step 5). `Flat`'s `FNode` keeps only `name` (`<module>/<node id>`), `origin` and `src`, not the frame (`core/ir/src/check.rs:109`), so a RuntimeId has to be computed from the IR modules, as the delta does. |
| **ID-16** | No derived id in a project file. | Holds; tested in `core/song/src/data/tests.rs`. |

### 2.4 Writes, delta, leases, diagnostics

| Rule | What it asks of step 7 | Today (checked) |
|---|---|---|
| **WRITE-8** | Delta names: module key / frame chain as call ranges / argument path / occurrence. "From step 7, SiteIds replace the mapped ranges." The delta drives the engine: values → `SetCell`, lanes → `SwapLane`, plugin states → `LoadState`, assets and the arrangement → prepared renders and `SetArrangement`, `muted` → its cell, structure → prepare and `Activate`. Today line: "`LoadState`, prepared renders and `SetArrangement` come in step 7". | Only `SetCell` and `SwapLane` are driven by the delta; everything else builds a renderer. |
| **WRITE-13** | Lane edits don't invalidate a knob drag; leases re-resolve at each `code_rev`. | Holds. |
| **WRITE-22** | At a `code_rev` change a lease that carries stays, "with its cells mapped into the new plan". | Leases are mapped to each new renderer's `CellTarget` (`lease.rs`), and a renderer built while a lease lives gets its value before it is sent. |
| **WRITE-22a** | Lane leases: `update` sends a preview `SwapLane` straight to the engine. | Not built (step 8b). |
| **DIAG-3** | `data.claimed`: a key swap within one change "can't be detected until tracks have SiteIds; from step 7 the notice also compares SiteIds". | Not built. |

### 2.5 PluginHost, homes, monitoring

| Rule | What it asks of step 7 | Today (checked) |
|---|---|---|
| **HOST-1, HOST-2** | Only through `PluginHost`, each call on its thread; main-thread calls on the process's real main thread. | Matches since 3b. |
| **HOST-5** | "From step 7 they persist in the arena by RuntimeId." | Carried by `CarryKey`. Not carried: plugins inside `poly` voices, an instance that asked for a restart, a changed state blob, and offline renders. |
| **HOST-6** | Transport, tempo, meter, loop, a transport event at each jump, `steady_time` from the engine clock. | Built (the `RunInfo` and `looping` sent to plugins, lib.rs:89-105, 431-440); the Today line is stale. |
| **HOME-4** | Precedence for a plugin parameter: a cable, then a code literal, then the blob. | `PluginNode.params` sends a `Const` once (when its last-sent value is `None`) and a cable when it changes (lib.rs:372-388). **A `LoadState` must clear the last-sent values**, or the blob's value wins over the code's until the next change. |
| **HOME-5a, HOME-6** | Code-owned parameters set back after a preset load; a saved state is never pushed back into the instance that produced it. | `load_state` skips a blob equal to the last save (inprocess.rs:277). Output events are collected, not consumed. |
| **MON-6** | A latch keyed by a ControlId, "built from SiteIds", or by plugin instance and CLAP parameter id. | Not built (needs plugin windows). |

### 2.6 Deferred rows, owner decisions, later steps that wait on step 7

- Owner decision 8: "Partitions and threads come right after the arena" (step 8a, RENDER-2's thread-count
  determinism, RENDER-32's per-partition Store fallback).
- Deferred: "Plugin windows, state autosave, plugin output events as automation, VST3 and AU" trigger
  includes "the arena keeps instances alive across plans".
- Deferred: "Latency compensation (RENDER-17 to RENDER-21) … Needs the arena."
- Deferred: "Mono, legato and glide (RENDER-16): With the arena's voice pools".
- Deferred: "Sending deltas to clients instead of refetching the whole state: With step 7, or when UI
  update latency is measured over budget."
- Deferred: "Exact range renders through kernel-only checkpoints (wasm memory snapshots cached by plan
  hash)": needs a plan hash; not step 7.
- Deferred: "Keyed iteration helpers and better structural matching": trigger "users hear state reset
  after refactors or loop reorders".

---

## 3. What earlier steps deferred to step 7

From the contract's Today lines, the code's comments and the step 6 design
(`.git/m2-step6-resume/design-6.md`):

1. **Kernels kept across edits.** RENDER-25 Today: "Kernels still start from init (step 7)". Every
   structural edit cuts every kernel tail (reverbs, delays, envelopes) behind a 5 ms crossfade.
2. **The arena replaces step 3b's carry.** `core/runtime/src/plugins.rs:15-16`: "Step 7's arena
   replaces `Carry`, `carry_from` and lending; `InstanceSource` and `PluginInstance`'s retire-on-drop
   stay." `CarryKey.ordinal`: "what tells unkeyed instances apart until step 7's RuntimeIds".
3. **`LoadState`.** `CarryKey.state`: "a changed blob is a new instance (no `LoadState` until step 7)";
   RENDER-26b; WRITE-8 Today; step 6 design §5.5 and §25.
4. **Prepared renders and `SetArrangement`.** WRITE-8 Today; step 6 design §5.5 ("`LoadState`,
   `SetArrangement` and prepared renders are step 7"). Today any arrangement, asset or audio-lane
   change builds a whole renderer.
5. **SiteIds in delta names.** WRITE-8: "From step 7, SiteIds replace the mapped ranges"; step 6
   design §5.2 and risk 1 ("a hand edit that reflows a whole track call renames everything in it, so
   the delta says 'structural' and the engine activates a renderer … step 7's SiteIds replace points").
6. **Cells across plans.** RENDER-23 Today: "cells live and die with their renderer until the arena
   keeps them across plans (step 7)".
7. **The wrap gesture's shape.** Step 6 design §14.4: the effect planner writes a wrap as two
   insertions so the wrapped call keeps its node names, "which is what step 7's arena needs to keep the
   wrapped instrument's state". Ready; nothing to do but use it.
8. **Swap of keys in one change.** DIAG-3; step 6 design §8.4: "Detecting a swap of keys within one
   change waits for SiteIds (step 7)."
9. **`key()` frames pinning SiteIds.** ID-7.
10. **Export reusing live instances.** RENDER-1 Today; HOST-5's "Not carried: … offline renders".
11. **Plugins inside `poly` voices** aren't carried (HOST-5 Today); RENDER-16 gives voices RuntimeIds
    by allocator slot.
12. **Store layout.** RENDER-32 Today: "one `Store` per renderer".
13. **Pending owner decisions the arena makes moot or changes:** step 3b's C7 (adopting a carried
    plugin's sounding notes, RENDER-26a Today), C3 (the PluginHost signatures as built, the `PluginHost`
    section); step 6's C17 (delta names in base-text coordinates, WRITE-8 Today), which matters if
    SiteIds were to replace points.
14. **Engine-side work the contract lists for step 7 but that has no built home yet:** the UI's
    "loading <plugin>" while a plan waits on plugin loads (RENDER-25).

---

## 4. Stale Today lines (checked against the code)

These describe `92a19a3`, not HEAD. A step 7 design should cite the code, not these lines, and the
step 7 commit should fix them (the contract's own rule: a step that changes behaviour updates its rules).

| Rule | Today line says | The code at HEAD |
|---|---|---|
| RENDER-6 | "one clock. `Block.start` is both" | Two clocks: `Engine.clock` and `Transport` (`engine.rs`, `transport.rs`); `Block { engine, runs }`. |
| RENDER-9 | `seek` resets and replays; cursors only move forward; plugins always "playing", 4/4 | Jumps are transport events (`Transport::apply`, `Jump`); `Renderer::seek` replays only offline. |
| RENDER-12, 13, 14 | notes are `SampleNote`; `poly` gates on the song sample | `Event::{On, Off, …}` (`event.rs`), schedulers (`sched.rs`), voices gate on events. |
| RENDER-17 | latency read "before state is loaded" | `create` loads the state, then `activate` measures latency (`host.rs`). |
| RENDER-25 | "when the 8-slot retired queue is full … dropped on the audio thread" | Engine stash (16), player `gone` (8), leak as last resort; never dropped on the audio thread. |
| RENDER-25a | "the new renderer replaces the old in one sample" | 5 ms linear whole-output crossfade (`engine.rs` `Fade`). |
| RENDER-27 | "the tempo map is read when a renderer is built" | Still true, but a tempo change already switches at one block line keeping the beat (`enter_from`, `Transport::retime`). |
| RENDER-28 (first sentences) | "a full queue drops the message"; "the audio thread allocates every block" | Senders retry up to 500 ms then report `Stalled`; the later Today sentences (3b, 5) are right. |
| RENDER-33 | "not yet on every pure fixture" | Every pure fixture and p16 (`core/play/tests/bit_identity.rs`). |
| HOST-6 | "always 'playing', 4/4 and no loop" | Built: tempo, meter, bar, loop, playing per run, `steady_time` from the engine clock. |
| HOME-4 | "a literal is sent once when the renderer starts and again after a reset" | Sent once per node build (`params` last-sent `None`); see 2.5 for what `LoadState` must add. |

---

## 5. The gate, and how to measure each item with `crates/gates`

What `crates/gates` has (read at HEAD):

- `Lab` (`src/lib.rs`): work folders, `compile`, `session`, `reference(&Reference::P50)`, `copy` of a
  fixture, `packages: Arc<PackageSet>`, `carry` (the `--no-carry` negative reference).
- The engine rig (`src/engine.rs`, feature `engine`): `m2_play::Player::driven` filled in simulated time.
  `Rig::{open, fill, send, prepare(song, Change), commit, applied, probe, plugins, ons, audio_work}`
  (`src/rig.rs`). `Deck` keeps the whole tape, `change` plays on during `prepare` for the wall time it
  took, `latency_ms` = prepare + frames until applied + one 512-frame buffer. `Probe` gives the engine
  clock, transport, `resets` (kernel resets), `generation`, held notes.
- Server gates (`src/server.rs`): a real `m2_server::graph::Graph` on the silent player. `Timed::add`
  takes `Graph::last_edit()` (`EditTiming { requested_ns, compiled_ns, sent_ns, published_ns, seq }`)
  and `heard_ms` = from `compiled_ns` (session's work done, compile included) to the start of the
  callback that applied `seq`, plus its block's frame, plus one device buffer. `Sampler` watches the
  playhead every 2 ms for a jump back. `open()` always starts playback at 1:00 (server.rs:42-52).
- `src/plugins.rs`: with-plugins checks (skip without Surge XT and Airwindows on `CLAP_PATH`), process
  `created()` counts, `Plugins` views.
- The probe plugin (`core/plugins/src/probe.rs`): a deterministic CLAP instrument and effect, inert
  until `install()`; records every main-thread call with its thread, notes with steady time, and has a
  state (`m2probe\0`, two parameters, a tag). It makes plugin gates runnable in CI.

### G7a. "In p50, a structural edit at 3:00 is audible within 200 ms after the compile"

- **Server gate** (the timed one): `server::add_effect` as it is, with two changes: an `--at SECONDS`
  option (`Opts.at`, default 60) so `open()` seeks there, and the budget passed per reference (step 3's
  100 ms on p16, step 7's 200 ms on p50). Run
  `m2-gates --server --reference 50 --at 180 add-effect`. Pass: every edit's `heard` ≤ 200 ms, no jump
  back (`Sampler`). Print the parts `Timed` already splits: the session's work (not counted), the
  server's part before it sends (this is where step 7 must win: only new instances built, not 6,000),
  the wait for the callback.
- Note for the design: `compiled_ns` is taken when the session's whole request is done, check and
  verify included. The Code row says "after the compile finishes", so the gate is slightly kinder than
  the words; say which in the step 7 design.
- **Engine rig twin** (that it is heard, and what the change cost on the audio thread):
  `gates::add_effect` with `--reference 50` and a start at 3:00 instead of `one_minute`. With the arena
  the twin should be a deck that is not activated at all, not "a twin that activates the song as it
  was", since a no-op activation must now change nothing (G7c). Pass: latency ≤ 200 ms; the second
  after the activation differs from the twin by more than -60 dBFS; nothing differs before it.
- **Plugin loads excluded**: the with-plugins half of `add_effect` already checks `created()` is
  unchanged across activations; keep it.

### G7b. "The tails of untouched tracks are intact"

A mixed output can't be split by track, so make the edit structural but silent, and require the whole
mix to stay bit-identical:

- **New engine-rig gate `tails`** on p50 from 3:00. Deck A: at 3:00 activate p50 with `t02`'s
  instrument wrapped in `vca(…, 1)` (`std/src/dsp.ts:133`: `kernel("std:scale", { in, by })`, an exact
  multiply by 1.0; written through a session Tx, or a save of `tracks/t02.ts`). Deck B: the same song,
  never activated. Both decks played with the same buffers.
- **Pass:** identical before the applied frame; inside the 5 ms fade window at most a rounding
  difference (the fade of `x·(1−g) + x·g` may differ from `x` by an ulp; bound it, e.g. 1e-6); from the
  applied frame plus the fade length on, `A.tape == B.tape` bit for bit for at least 5 s, which covers
  `t03`'s reverb and `t01`'s delay tails and every voice sounding at 3:00. Also `Probe.resets`
  unchanged, and new arena counters (below) say every old instance was kept and one was created.
- **Negative control:** the same run with the arena off (every activation creates every instance, the
  successor of today's `--no-carry`, or HEAD's engine) must fail it: its tape differs from B after the
  fade by far more than -60 dBFS. A gate that passes on HEAD measures nothing.
- **An audible edit too:** `t02` gets a `delay` (G5's edit). Its untouched tracks can't be separated
  in the mix, so judge it as G5 does (heard) and rely on the silent edit for the bits.
- **New counters the rig needs:** `Probe` gains `kept` and `created` for the last activation (kernel
  instances, plugin instances, feedback rings, voice pools), read from the engine like `resets`.
- Keep step 3's G6 (`gates::step`: no sample-to-sample step over twice a sine's own across an
  activation): it is what proves a per-edge fade (section 6, item 5) is still click-free.

### G7c. "Live playback with no edits is bit-identical to offline"

- Already a CI test: `core/play/tests/bit_identity.rs` (`live_is_the_export_on_every_pure_fixture`,
  `live_is_the_export_on_p16`, ignored `live_is_the_export_on_all_of_p16`). Step 7 rewrites what both
  paths run, so these must pass unchanged with the new `Plan` and the chosen Store layout.
- **Add, next to `a_no_op_swap_changes_no_bit`:** `a_no_op_activation_changes_no_bit`: play first-song
  and p16, and at 5 s activate a plan built from the same revision; the output must equal offline bit
  for bit, fade window included (every instance kept, nothing to fade). This is the cheapest proof
  that the arena keeps state exactly.
- **Add a recorded p50 run** (`--ignored`, as p16's), since the gate names p50 for the edit and the
  Store layout may differ at that size.
- In `crates/gates`, the existing engine rig already asserts RENDER-33 as every gate's starting point
  (`src/engine.rs:174`); nothing more is needed there.

### G7d. "A kernel module rebuilt with the same kernel id but a new hash starts from init"

- **Runtime test** (core/runtime or core/play), since the app has no way to reload a package inside a
  session (step 6 design: "packages don't change within a session"): load std; make a second
  `PackageSet` from a copy of std whose wasm file has one extra custom section (valid wasm, same code,
  new SHA-256). Play a delay with feedback; mid-echo, activate the same `Flat` built with the second
  set. Pass: arena counters say no std kernel was kept; after the fade the echo is gone (the new delay
  starts empty), which the twin activated with the original set keeps (bit-identical to never
  activating).
- **Control:** a third `PackageSet` loaded from the same, unchanged bytes (a new wasmtime `Module`, the
  same hash) must keep every instance: the shape compares the content hash, not the `Module` object.
- **In `crates/gates`** (optional, same check at p50 scale): a `rebuilt` gate whose `Rig::prepare` takes
  a `Change::Packages(Arc<PackageSet>)`, with the lab writing the custom-section copy of std under
  `lab.work`.
- Note: std is one wasm module for all kernels, so "the kernel's module hash" is std's wasm hash; every
  std kernel restarts when any of them is rebuilt. Each `PackageSet` also has its own wasmtime `Engine`:
  keeping old instances next to new ones from another set only works when their Stores are separate
  (per instance, or per set).

### G7e. "An export of `examples/with-plugins` pauses playback, reuses its instances, and playback resumes with their state intact"

- **Server gate `export`** (`--server`), skipped without Surge XT and Airwindows on `CLAP_PATH`, as
  `with_plugins` is. Open the copy, play from 0:02, export (through what the app calls,
  `m2_server::export::start`, or the `Graph` call it uses).
  - *Pauses:* while it runs the published status is not playing and the state says why (RENDER-1: "the
    UI says so"); the engine processes no plugin (the probe's process count, below, or the instances'
    `take_process_request`/status).
  - *Reuses:* `created()` unchanged across the export, and the playing plan's plugin handles before and
    after are the same `Handle`s.
  - *State intact:* `save_state` on each handle before the export and after it give the same bytes
    (Surge's state includes its patch, not voices; equal bytes is the check).
  - *Resumes:* status playing again, at the position it held, and audible within a buffer or two.
  - *The export itself:* a WAV of the song's length; plugins aren't bit-exact between instances, so
    compare its level and onsets to a CLI render (`m2 render`, fresh instances), not bits.
- **CI version with the probe** (no Surge needed): a project that uses `clap:m2.probe` with a saved state.
  The probe's call log must show, on the plugin main thread and in this order, `deactivate`, `reset`,
  `load_state` (the committed blob), `set_render_mode(Offline)`, `activate`, then after the render
  `set_render_mode(Realtime)`, `load_state` (the state it had), `activate`; no `process` call from the
  audio thread between the first `deactivate` and the last `activate`; and, the probe being
  deterministic, the export with reused instances must equal the export with fresh instances bit for
  bit (this is what proves `reset` + `load_state` give RENDER-1's exact start).
- **A pure song** (`examples/first-song`) exports beside playback: no pause, no state change (RENDER-1's
  "A pure song's export runs beside playback").

### G7f. "The per-instance Store cost at 6,176 instances is measured, and the store layout is chosen from it"

- **New gate `store`** (engine-level, no server, no device): compile p50, and build its instances in each
  layout under test:
  - one Store per renderer (HEAD's);
  - one Store per instance (RENDER-32's first choice);
  - one Store per module (the closest thing to "per partition" before step 8a), or one Store per
    prepare batch (section 6, item 8).
- **Count first:** kernel instances = kernel nodes at top level plus, for each `poly`, voices × kernel
  nodes in its voice graph. Print it and compare with the contract's 6,176 (the gate must state the
  count it measured, and the contract's number is fixed from it).
- **Measure per layout:** build time (instantiate + `init` + prewarm), total and per instance; memory
  (sum of wasm memory sizes, and the process footprint before and after, `task_info` phys footprint on
  macOS); the time to process one 128-frame block of p50 at 3:00 over 30 s of song (mean, p99, max), as
  a share of the 2.67 ms block budget; the time to prepare a one-instance edit; the time to drop a
  retired plan off the audio thread.
- **Choose:** per instance unless its p99 block time or its memory is worse than per renderer by a
  margin the design states up front (for example 5% of block time, or 50 MB). Record the run in the
  contract's RENDER-32 Today line.

### Gates that must keep passing

All of step 3's (G1 to G7, G10, with-plugins, G8, G9, G11): the arena replaces `Carry`, so the
with-plugins "carried instances" checks become "kept by the arena" checks with the same meaning, and
`--no-carry` becomes the arena-off negative reference. `core/play/tests/no_alloc.rs` must run an
activation that keeps and replaces kernels and plugins, with retiring nodes, under the allocation trap.

---

## 6. Rules whose complexity looks unnecessary

Each with the reason and a proposed edit. None weakens a guarantee a producer hears.

1. **ID-13's session-assigned `SiteId(u32)` and its four matching rules (also ID-7, WRITE-8, DIAG-3,
   MON-6's Why).** Step 6 already names every node by module key, frame chain (each frame its call's
   place, or `key:<k>`), what it is, and occurrence, in reference coordinates that an insertion
   elsewhere can't shift (`core/session/src/tx/delta.rs`). Between what plays and the accepted revision
   that name *is* InstancePath / RuntimeId, computed pairwise, deterministically, by code that exists
   and is tested. A `SiteId(u32)` assigned by matching each accepted revision against the previous one
   is session history: put into delta names (WRITE-8's "From step 7, SiteIds replace the mapped
   ranges"), it would make `DeltaHash` differ between two processes with different histories, which
   breaks step 6's gate ("the same hash in two processes").
   *Edit:* ID-13 becomes "a site is matched across two revisions by the delta's names (WRITE-8): the
   Tx's own edits, `key()` frames, and WRITE-12's carry; an unmatched site is new". Drop item 4 (the
   structural match ignoring bindings and literals: a guess, the riskiest of the four, and the one ID-13's
   own Why warns against) into the existing deferred row "better structural matching". Drop "token-level
   Myers" for WRITE-12's carry. Delete WRITE-8's last sentence. ID-7: "a `key()` frame is named by its
   key". DIAG-3: compare each module's `track()` call place pairwise, or defer the key-swap notice.
   MON-6's Why: "a ControlId is per build".
2. **`LoadState` as an engine message (Engine shape, RENDER-26b, RENDER-28, Planes table).** CLAP's
   `state.load` is a main-thread call the plugin must make safe while it processes, and the in-process
   host already runs it on the main thread with no audio-thread handshake (`inprocess.rs:275`). The
   engine needs only to re-send code-owned parameters afterwards (HOME-4), which a flag on the instance
   read at the next block does.
   *Edit:* RENDER-26b: "the host loads the blob on the plugin main thread … and the instance's code-owned
   parameters are sent again at its next block (HOME-4)". Remove `LoadState` from `Msg` and from
   RENDER-28's list.
3. **`SetArrangement` as its own message.** Once activation keeps every matching instance, an
   arrangement change is an activation whose structure is unchanged: new timing tables and clip renders
   prepared off the audio thread, switched at one block line, every instance kept, the beat kept (what
   `enter_from` already does). Audio-lane edits are a `SwapLane` whose data is the prepared clip renders.
   *Edit:* the Arrangement row's path becomes "prepared off the audio thread; `Activate` with the same
   plan"; the prepared-data row "`SwapLane` (audio lanes with their renders)". The message set stays what
   it is today: `Activate`, `SwapLane`, `Transport`, `Panic` (and `Live` later).
4. **`Engine { arena: Map<RuntimeId, Slot> }` on the audio thread.** Matching by RuntimeId can be done
   entirely off the audio thread when the plan is prepared against the plan that plays; `Activate` then
   carries a move list `(old slot → new slot)` and the audio thread only moves boxed instances between
   pre-sized slot vectors. RuntimeIds live in the server's record of what plays (`Plays`), not in the
   engine.
   *Edit:* the Engine shape's `arena` becomes "the instances of the playing plan, by slot"; RENDER-25
   adds "matched when the plan is prepared".
5. **RENDER-25a's fade "at every module output whose subgraph changed", with old and new running
   together.** A module whose chain changed still holds kept instances (the `pluck` an effect was added
   after). Running the old module subgraph for the fade would process those kept instances twice, which
   is why step 3b needed lending (`plugins::lend`, `render_block_lent`) and would need it for kernels too.
   Fading at the edges instead needs none of it: every input whose source changed ramps from the old
   source to the new over 5 ms, and replaced nodes run on for one fade window as retiring nodes in the
   new plan (reading kept upstream nodes), then go. An added or removed effect is exactly "an input whose
   source changed". No instance is ever processed twice.
   *Edit:* RENDER-25a: "An activation crossfades over 5 ms every input whose source changed; a replaced
   node runs on beside its replacement for that window. Nothing else is faded." G6 keeps measuring clicks.
6. **RENDER-26a's adoption (step 3b's C7) and the carry machinery.** If lane schedulers belong to the
   engine, keyed by `LaneAddr` and kept across plans (lanes are addressed by module and lane key, ID-6),
   a kept consumer simply keeps receiving its lane's events. Then `NoteScheduler::adopt` and
   `raise_next_id`, `PluginInstance.next_id` and `held_cap`, `PluginNode.release` for carried notes,
   `carry_from`, `lend`, `Carry`, `CarryKey`, `render_block_lent`, the whole-renderer `Fade`, and
   `SwapLane`'s renderer `generation` guard (a swap names its `LaneAddr`) can all be deleted.
   *Edit:* RENDER-26a: "Schedulers are kept across activations. An activation that replaces a note
   consumer first sends an Off for every note sounding into it; a kept consumer keeps its notes." C7 is
   withdrawn.
7. **RENDER-26b's and RENDER-17's live latency re-plan.** Compensation isn't built, and nonzero latency
   is refused at check time (RENDER-21, `check.rs:680`). A plan-only activation that re-plans delays has
   nothing to re-plan in step 7.
   *Edit:* until compensation ships, a `LoadState` (or restart) that changes a plugin's latency raises a
   diagnostic and plays uncompensated, as RENDER-21's refusal already implies; `Plan.latency` waits for
   compensation. Keep RENDER-17's text for that turn.
8. **RENDER-32's fallback "one `Store` per partition".** Partitions don't exist until 8a, and wasmtime
   frees an instance's memory only when its Store is dropped, so any shared Store keeps the memory of
   replaced instances until every instance in it is gone. The real choices are per instance, or (if that
   is measured too costly) one Store per prepare batch: the open's batch holds everything, each edit's
   batch holds its few new instances, and a batch's Store is dropped when its last instance retires. That
   leaks only what an edit replaced inside a batch still partly alive, bounded and visible.
   *Edit:* RENDER-32's fallback names what is measured (G7f), not partitions.
9. **RENDER-28's "or coalesces to the newest plan".** A move list is relative to the plan it was
   prepared against; coalescing past a plan that never played would apply it to the wrong slots. One
   activation in flight, the next prepared against the acknowledged one, is simpler and the actor is
   already serial.
   *Edit:* "Structural messages are never dropped; at most one `Activate` is in flight, and the next plan
   is prepared against the plan the engine acknowledged."
10. **HOST-5's carry key.** A keyed plugin's state path (module, frames, key) is saved identity and unique
    by `plugin.state-collision`; it is a better RuntimeId than any position, and survives reorders.
    *Edit:* HOST-5: "a keyed plugin's RuntimeId is its state path; an unkeyed one's is ID-15's".
    A plugin that asked for a restart stays "a new instance at the next activation" (today's rule), which
    needs no engine handshake.
11. **Partial matching inside `poly` voice graphs.** Owner decision 1 and RENDER-26a already accept that
    a replaced note consumer ends its notes. Matching instances inside each voice slot of a changed voice
    graph buys little and costs per-voice move lists.
    *Edit:* RENDER-25: "a `poly` whose voice graph or allocator settings changed is replaced whole".
    Keep "a voice's RuntimeId is its allocator slot" for mono and legato later.
12. **The deferred "Sending deltas to clients … With step 7".** A UI concern with its own trigger; tying
    it to the engine step only enlarges step 7.
    *Edit:* trigger becomes "UI update latency measured over budget".
13. **The actor's cell plumbing (step 5 and 6).** `same_layout`, `value_writes`' bare-`Flat` comparison
    and `Open::adopt` (actor.rs:151-205, 335-356) exist because cells are found by renderer index. If the
    plan carries a map from delta name to `CellId`, a values-only delta writes cells by name, and a value
    with no cell (a const native code made, which the checker folds) activates. RENDER-23 itself stays.
14. **Kept as is, though they look heavy:** ID-14 (cheap to add to the matcher, and it stops a loop
    reorder from handing one instance's state to its neighbour); the wasm hash in the shape (one hash
    compare per instance); RENDER-1's instance reuse for export (memory of sample-based instruments and
    licence limits are real; the pause is the stated cost). For RENDER-1, the simplest form is a whole
    engine pause (an engine message applied at once, like stop, that makes it output silence and process
    nothing, acknowledged by its `Applied` stamp) rather than a per-instance handoff.

What the arena must keep, spelled out (the contract says "instance"): kernel instances, plugin instances,
`poly` nodes (voices and allocator), `Feedback` rings (a reset ring is a 128-sample hole in a feedback
loop), audio-lane nodes (their jump taps), and note schedulers (by `LaneAddr`). The plan itself
(order, buffers, cell table, kernel input versions) is new each time.

---

## 7. Guarantees that must not move, and where they are tested today

| Guarantee | Rule | Test that must stay green |
|---|---|---|
| No clicks at an activation | RENDER-25a | gates G6 `step`; with-plugins G5 step check |
| No stuck notes | RENDER-13, 26a | gates G4 `stuck`, with-plugins G4 |
| Tails ring through jumps and stop | RENDER-9, 6a | gates G2 `loop`, G7 `tail-stop` |
| Tails intact through edits | RENDER-25 | new G7b `tails` |
| Export exact and deterministic | RENDER-1, 2 | `core/session/tests/renders.json` fixtures; G7e's probe export equals a fresh one |
| Live equals offline | RENDER-33 | `core/play/tests/bit_identity.rs`, plus `a_no_op_activation_changes_no_bit` |
| No audio-thread allocation or lock | RENDER-28 | `core/play/tests/no_alloc.rs`, extended to an activation with moves and retiring nodes |
| No jump back on edits | RENDER-8 | G5 (rig and server `Sampler`) |
| Plugin calls on their threads | HOST-1, 2 | `core/plugins/tests/main_thread.rs`, the probe's thread log |
