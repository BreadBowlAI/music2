# Today's engine, mapped for step 7

A read-only survey of HEAD `cbf5eb9`. `core/runtime`, `core/play`, `core/plugins`, `core/kernel` and
`core/ir` are unchanged in the working tree, so their pointers hold for both. `crates/server` and
`core/session` are being edited by another workflow, so their pointers are to HEAD (`git show HEAD:…`).
Nothing was built or run. Each cost marked *estimate* comes from the code and one recorded
measurement; section 12 lists what to measure first.

---

## 0. The short version

- **One `Renderer` is everything.** It holds the plan (the `Flat` turned into a `Graph`), the data
  (lane schedulers, control lanes, audio clip renders, the tempo map, the arrangement), every kernel
  instance and its wasmtime `Store`, every `poly` voice, the cells and their slots, and the
  transport. It is built whole, played whole and retired whole (`core/runtime/src/lib.rs:1237`).
- **A structural edit rebuilds the whole song.** That means anything other than a cell write or a
  lane swap: code, an audio lane, a plugin state, an asset, the arrangement, or a notes lane that
  outgrew its scheduler. The session actor runs `Renderer::build_live` on its own thread. That build
  makes a new `Store`, then instantiates the entire `std.wasm` module once for each kernel node in
  each voice (about 6,060 to 6,176 instances in p50, about 97% of them inside `poly` voices). It
  inits and resets every one. It rebuilds every notes lane's `NotesData`, takes audio clip renders
  from the cache, and carries or creates plugins. On the audio thread, the old and the new renderer
  then both run whole for 5 ms (two blocks), and the old one is dropped on `m2-play-drop`.
- **What every structural edit loses, on every track:**
  - every kernel's state: reverb and delay tails, oscillator phases, filter memory, envelopes;
  - every note that is sounding. The old renderer sends an Off and fades out in 5 ms, and the new
    one doesn't chase notes;
  - feedback rings;
  - audio-lane fade state.

  What survives:
  - the transport: beat, play state and loop;
  - the engine clock;
  - decoded assets and clip renders, through `audio::Cache`;
  - plugin instances whose `CarryKey` matches, with their tails, and with their notes when the same
    lane feeds them.
- **So step 7's gate fails today on every structural edit:** "the tails of untouched tracks are
  intact" doesn't hold for any track. The 200 ms gate is borderline. The reference project measured
  about 54 ms from compile to audible (step 3a's commit message; most likely p16). Scaling by
  instance count puts p50's build alone at 150 to 170 ms (*estimate*).
- **Several pieces step 7 needs already exist:**
  - `m2_song::DataSet` (the contract's `DataSet`).
  - Step 6's `GraphDelta` already names nodes across two revisions in shared text coordinates
    (`core/session/src/tx/delta.rs`). That is ID-13's rules 1 to 3 in practice.
  - Per-cell slots.
  - `PluginHost::load_state`, `set_render_mode` and `deactivate` exist and are tested in
    `core/plugins`, but the runtime never calls them.
  - The engine clock and the transport are separate.
  - `SwapLane`, and a retire path that never frees on the audio thread.

---

## 1. Data flow, end to end

```
files ─► Session (core/session)
           compile: QuickJS build → Ir (modules, nodes, frames, origins) → m2_ir::check → Flat
           Project::data(decls) → m2_song::DataSet (arrangement, tempo, lanes, states, assets)
           Revision { compiled: Compiled { code: Arc<Code{ ir, flat, sites, … }>, data, problems } }
        │
        ▼  actor settle (crates/server/src/graph/actor.rs @HEAD, Open::way at 409)
   delta_between(plays.revision, accepted)                       core/session/src/tx/mod.rs:540
        ├─ Reach: cell writes (value_writes, same_layout) + SwapLane per lane      actor.rs:151-211, 376
        └─ else: build() → Renderer::build_live(flat, data, packages, rate,
                              audio::Cache, Owner, InstanceSource = Carry | Fresh)  actor.rs:294
        ▼
   Player::activate(renderer)           core/play/src/lib.rs:686   (strict off, cells.sending)
        ▼  crossbeam bounded(QUEUE=1024), try_send, retry ≤ 500 ms then `Stalled`
   audio callback: Callback::fill → Live::fill → [Resampler] → Engine::process
        Engine::block: stash → finished fade → head/blocked → inbox (messages at the block line)
        Engine::render: current.render_block(clock) [+ fading.render_block_lent + linear fade]
              Renderer::render_block_lent: cells.apply → schedulers.run → block() → Graph::process
        ▼
   retired objects → bounded(RETIRED=64) → `m2-play-drop` thread (polls every 5 ms) drops them
```

Offline: `m2_editor::render::Render::new` calls `Renderer::new`, which builds with `Fresh` plugins and
a throwaway owner, then `seek(first)` replays from 0, then `process` writes the WAV. A range render
uses `Renderer::starting_at` (`crates/editor/src/render.rs:131-170` @HEAD). Export Audio runs that
`Render` on its own `music2-export` thread while playback goes on
(`crates/server/src/export.rs:96-140` @HEAD, built by `Actor::render`, actor.rs:889).

---

## 2. The Renderer (`core/runtime/src/lib.rs`)

| Field | What it is | Lives |
|---|---|---|
| `store: Store<StoreLimits>` | the one wasmtime store holding every kernel instance of this renderer | with the renderer |
| `graph: Graph` | the top-level `Exec` per Flat node, buffers, const versions | with the renderer |
| `cells: Cells` | cell table, `Arc<CellSlots>`, `Arc<CellMap>`, ramps, heard seqs | with the renderer |
| `lanes: Vec<LaneSlot>`, `slots: Vec<LaneRef>` | one scheduler or lane per distinct lane read, numbered by `lane_slots(flat)` | with the renderer |
| `tempo: TempoMap`, `arrangement: Arc<Arrangement>` | cloned from the DataSet at build | with the renderer |
| `transport: Transport`, `clock`, `runs`, `pending` | the song position; the next block's engine clock | copied to the next renderer at activation |
| `generation` | unique id; `SwapLane` must name it | with the renderer |
| `plugins`, `broken`, `strict`, `trace`, counters | flags | with the renderer |

### 2.1 Flat: the only "plan" today

`m2_ir::check(ir, kernels) -> Flat` (`core/ir/src/check.rs:134`, `run` at 464):

- `Flat { nodes: Vec<FNode>, order: Vec<usize>, out: PortRef, out_kind, sidechain }`.
- `FNode { name: "<module key>/<node id>", module, src, origin: Option<Origin>, op: FOp, outputs }`.
- `FOp` = `Lane{module, lane, kind, default}` | `Time` | `Const(f64)` |
  `Kernel{kernel, channels, inputs: Vec<Input>}` | `Plugin{plugin, inputs, channels, state}` |
  `Poly{lane, voices, voice: Box<Voice>, channels}` | `Feedback{input, channels}` | `NoteField` |
  `Capture(PortRef)`.
- `Input` = `Port{src, src_channels, channels}` | `Const(f64)` | `Unset`.
- How a Flat is made:
  - Every non-import node of every module, in module order, gets an index, and imports are resolved
    away.
  - The order is Kahn's algorithm with the lowest index first, so it is deterministic. A feedback
    input doesn't count toward the order.
  - Nodes the output doesn't reach are dropped from `order`.
  - A const without an origin is folded into its reader's `Input::Const`.
  - Inside a voice, a ref to an outer node becomes a `Capture` node appended after the voice's own
    nodes.
- **What it lacks for step 7.** `FNode` carries no frame: `Node.frame` and `Module.frames` stay in
  the `Ir`. Node indices and names are local to one build (ID-12). There is no latency planning:
  RENDER-18 isn't built, and RENDER-21 refuses latency at check.
- **There is no separate planner.** `Builder::graph` (lib.rs:596) turns the Flat into a `Graph` one
  for one: one `Exec` per node, and a `Vec<f32>` buffer per output port (`channels × 128`).

### 2.2 Graph and Exec

`Graph { exec: Vec<Exec>, order, bufs: Vec<Vec<Buf>>, versions: Vec<u64>, version, resets }`
(lib.rs:553). The state each `Exec` holds (lib.rs:529):

| Exec | State |
|---|---|
| `Control{slot, default}` | none: `ControlLane::value_at(beat)` per sample |
| `Audio(AudioNode)` | fade taps, fade-in, continuation position (audio.rs:380) |
| `Notes` | none here: the lane's `NoteScheduler` runs before the graph |
| `Time` | none |
| `Const(f32)` | its value, which is the cell's (`write_const` bumps `versions[node]`) |
| `Kernel(Box<KernelNode>)` | a wasm instance: memory, typed funcs, input ptrs, `seen` versions |
| `Plugin(Box<PluginNode>)` | `Arc<PluginInstance>`, note keys, `ended`, `release`, params last sent, event lists, `lent`, status |
| `Missing(Option<Input>)` | none: passes its input through, or plays silence |
| `Poly(Box<PolyNode>)` | voice slots, each with its own `Graph`; `born`; `plan` |
| `Feedback{input, ring}` | one block of samples, indexed by engine clock mod 128 |
| `NoteField`, `Capture` | none |

`Graph::process` (lib.rs:847) handles nodes in `order`. A kernel's `Input::Port` is copied into its
wasm memory only when the source's version differs from what the kernel last saw. A steady const
costs nothing per block (RENDER-22). Then `k_process` runs, and its outputs are copied out. In strict
mode a non-finite sample is an error. Feedback rings are written after the pass.

### 2.3 Kernel instances, the Store and linear memory

- **One `Store<StoreLimits>` per renderer** (lib.rs:1293-1296). The limits are 32 MiB per memory
  (`KERNEL_MEMORY`) and 1,000,000 instances, memories and tables per store. wasmtime's default of
  10,000 per store would cap a song at 10,000 instances; p50 is about 6,100, and a 99-track song
  would pass the cap.
- **One wasm instance per kernel node per voice.** `Builder::kernel` (lib.rs:754) runs
  `Instance::new(store, module, &[])` on the **package's whole module**.
  - The module is `std/kernels/std.wasm`: 143 KB, 304 functions, 176 exports, 29 kernels, one
    memory with a minimum of 2 pages (128 KiB), about 9.8 KB of data, and a 64 KiB stack set by
    `std/kernels-src/.cargo/config.toml`. I parsed the module's sections to get these numbers.
  - Then `get_typed_func` six times by `"{prefix}_{name}"` (each name `format!`ed) and
    `get_memory`.
  - Then `k_init(rate, 128, channels)`, the `k_ports` check against the manifest, `k_port(i)` for
    each port with its bounds and alignment checked, and the ABI-1 overlap check (a sort).
  - Last, `Input::Const` values are written into input memory once.
- **Why nothing kernel-side survives a rebuild.** A `KernelNode` holds a `Memory` and `TypedFunc`s,
  and those handles are valid only in their `Store`. An instance can't move to another renderer's
  store.
- **wasmtime 49.0.2 facts** (workspace features `anyhow, cranelift, runtime, std`, so no pooling
  allocator):
  - Instances use the on-demand allocator. Each linear memory reserves 4 GiB of address space,
    plus a 32 MiB guard after it and one before it (`Tunables::default_u64`). In p50 that is about
    25 TiB of virtual address space, twice that during an activation (*estimate*, from the
    defaults).
  - On macOS a module compiled from bytes gets no copy-on-write memory image, so each instantiation
    copies the data segments.
  - A `Store` frees nothing until it is dropped (store.rs:139-141: it is "intended to be a
    short-lived object" and "will not be deallocated until the Store itself is dropped"). **This is
    the real reason behind RENDER-32.** One long-lived arena store would leak every replaced
    instance. Per-instance stores, or per-partition stores rebuilt from time to time, are the ways
    out.
- **Prewarm** (lib.rs:1006, called at 1322) resets every kernel once after the build. `k_reset`
  rebuilds `K::new`, which may grow the wasm heap. Doing it here means a later reset on the audio
  thread (a panic, a voice retrigger) never grows memory. Tests: `prewarming_keeps_kernel_memory_flat`,
  and no_alloc's `kernel_memory_bytes` compare.
- **The ABI-1 debug check** copies every input port before each `k_process` and `k_reset` and
  compares after (lib.rs:166-212).
- **Memory size.** `kernel_memory_bytes` reports `data_size`, the accessible bytes. That is at least
  128 KiB per instance, so at least about 790 MB accessible in p50. RSS is lower: arm64 macOS pages
  are 16 KiB and only touched pages commit (*estimate*, unmeasured).

### 2.4 Voices (`poly`)

- `PolyNode { slot, voices: Vec<VoiceSlot>, out, born, plan }`.
- `VoiceSlot { graph: Graph, note: Option<VoiceNote>, born, dirty, last_peak, taken }`
  (lib.rs:221-251).
- **Every voice is built eagerly** at renderer build. Each voice graph has its own `KernelNode`s,
  so a 12-voice pluck is 12 × 13 instances.
- **Allocation:** the first free voice, else the oldest by `born` (lib.rs:1098-1114). A voice's
  graph is split into segments at the frame its new note starts. A reused voice that is `dirty` is
  `reset`, which calls `k_reset` on the audio thread (RENDER-10 and RENDER-16's one policy).
- **A voice ends** when its Off has passed, its block peak is under −80 dB and no kernel in it
  reports `active`.
- **The gate is on the engine clock:** `VoiceNote.off` is an engine sample (RENDER-13).
- A cell inside a voice is `CellAt::Voice{poly, node}` and is written into every voice.

Kernel instances in p50, counted from std's sources (`std/src/instruments.ts`, `dsp.ts`, `math.ts`,
`mix.ts`; `core/session/src/reference.rs` writes p50):

| Track type (count) | Per voice | Voices | Top level | Per track | Total |
|---|---|---|---|---|---|
| `delay(bass(…))` (17) | 14 | 4 | drive, delay | 58 | 986 |
| `pluck(…)` (17) | 13 | 12 | none | 156 | 2,652 |
| `reverb(pad(…))` (16) | 12 | 12 | reverb | 145 | 2,320 |
| mixer + master | | | 50 gain, 49 add, 1 limit | | 100 |
| **total** | | | | | **≈6,060** (contract: 6,176) |

About 5,900 of these, 97%, are voice instances. I don't account for the contract's extra roughly 120;
the shape of the result is the same either way. **The voice pools are the arena's main
population.**

### 2.5 Lanes and schedulers

- `LaneSlot = Notes(NoteScheduler) | Control(Arc<ControlLane>) | Audio(AudioLane)` (lib.rs:480).
  Slots are numbered by `lane_slots(flat)` in node order (lib.rs:507). `SwapLane` names the slot
  number and the renderer's `generation`.
- **`NotesData`** (`sched.rs`) is built off the audio thread. It holds the notes in samples, using
  the tempo map at build time, so a tempo change needs every `NotesData` rebuilt. It also holds
  `by_key` (note identity `(clip, pitch, start beat)` plus its ordinal among equals) and `needs:
  Caps{sounding, ons}`.
- **`NoteScheduler`** holds `data: Arc<NotesData>`, `cursor`, `sounding: Vec<Sounding>`, `next_id`,
  the flags `swapped`, `released` and `adopted`, `events: Events`, and `caps`. Its room is at least
  `Caps::FLOOR` (256 sounding, 128 ons), so most edits swap in place.
  - **`run`** handles a jump other than `Enter` by sending an Off for each sounding note and seeking
    the cursor. On `Enter` (a renderer's first block) it only seeks. After a swap, `reconcile`
    (RENDER-26a) runs first, then `play`.
  - **`release_all`**, on the old renderer's first faded block, sends an Off for everything and
    starts nothing more.
  - **A swap is refused** when `needs > caps`, and then the server rebuilds the renderer.
- **Control lanes** are `ControlLane::value_at(beat)` per sample, with no state.
- **Audio lanes** are `AudioLane`, one `Arc<RenderedClip>` per clip, rendered whole into memory.
  They come through `audio::Cache`, keyed by asset SHA, clip fields, tempo segments and rate
  (audio.rs:98-158). Because the tempo is in every clip's key, a tempo change re-renders every
  clip. The cache is shared by every build of a session, and `prune()` keeps what the last build
  used.
- **`AudioNode`** keeps up to `TAPS` fading taps for the 5 ms equal-power crossfades at a seek, a
  wrap, a stop or a play (RENDER-9 item 5). Its first block in a new renderer fades nothing, because
  the whole-output crossfade covers it.

### 2.6 Event buffers

- `event::Events` is a fixed-capacity `Vec<Event>`. A full buffer refuses an On and counts it; an
  Off always fits, because buffers are sized from `Caps::events()`.
- Events are sorted per block, Offs first at equal frames.
- `Event` is RENDER-14's full enum. Only `On` and `Off` are produced.
- `poly` reads its lane's buffer directly.
- A plugin node translates the buffer into `m2_plugins::Events`, which has CLAP's layout
  (`plugin_events`, lib.rs:314). Into the same list it merges parameters sampled every 16 frames,
  and a choke plus a NoteOff for each note in `ended` (after a panic) and a NoteOff for each in
  `release` (notes a carried instance held that the new node didn't adopt).

### 2.7 Cells

- **Which constants are cells.** `cells(flat)` (cells.rs:47) lists every const with an origin that
  the order reaches, top level first, then each poly's voice graph. A `CellId` is an index into this
  table, and means something only within its renderer (RENDER-23).
- **`CellSlots`**: one `AtomicU32` value and one dirty bit per cell, plus `published` and `any`.
  Writers are serialised by `core/play/src/cells.rs`'s `Writer` lock; seqs start at `CELL_SEQ =
  2^48`. The audio thread drains at each block line (`Cells::apply`). A step write calls
  `write_const`, which bumps the node's version, so each kernel it feeds rewrites its input once. A
  ramp is copied for one block and ends on its value (RENDER-22a).
- **`CellMap`** maps an `Origin` (by `same_value`, with build-local site ids) to `CellId`s. A
  `CellTarget` is `(Arc<CellSlots>, Arc<[CellId]>)`.
- **On the server** (actor.rs @HEAD):
  - A value edit is written into what plays only when `same_layout(old, new)` holds (cells at the
    same `CellAt` node indices) and `value_writes` finds that only const values differ; otherwise the
    whole renderer is rebuilt.
  - A new renderer gets each live lease's value set in its cells before it is sent
    (`Renderer::set_cells`, actor.rs:575-583).
  - Leases are aimed again at the new renderer's slots (`PlaysView::target_of`, lease.rs:388).

### 2.8 Plugin nodes

- **`PluginNode`** (lib.rs:254) holds:
  - `inst: Arc<PluginInstance>`, `audio_in`, `notes: Option<slot>`;
  - `keys`, the notes sounding into it by id;
  - `ended`, `release`, and `params` as `(id, Input, last)`;
  - the event lists in and out, sized at build;
  - `lent: Option<usize>`, `status`, and counters.
- **One `host.process` call per transport run,** so a call never spans a jump. Each call gets that
  run's transport: beats, seconds, bpm, playing, loop, meter, bar, and `steady` from the engine
  clock (HOST-6 is in place).
- A **const parameter** is sent once (`last == None`), and again after a reset.
- **Idling.** `can_idle` and `woken` read each top-level plugin's last `Status` and
  `take_process_request`.

### 2.9 The other nodes

- **`Feedback`**: a one-block ring indexed by `(engine + frame) % 128`, written after each pass.
- **`Missing`**: an uninstalled plugin (FMT-18a), which passes its audio input through or plays
  silence.
- **`Time`**: copies `beats` or `seconds` from the transport.
- **`NoteField`** and **`Capture`**: inside voices only.

---

## 3. The live engine (`core/runtime/src/engine.rs`) and the player (`core/play`)

### 3.1 The two clocks

- **The engine clock** is `Engine.clock`, a `u64`, passed into each `render_block(clock, …)`. It
  drives the 128-frame block grid, the feedback ring index, voice gates, CLAP `steady_time` and the
  applied stamps. It never resets: `Engine::with(renderer)` starts from the renderer's own clock.
- **The transport** is owned by each Renderer (`Renderer.transport`, `transport.rs`).
  - Its position is an integer frame. Each block is at most 2 `Run`s, each tagged with a `Jump`
    (`None`, `Start`, `Enter`, `Seek`, `Wrap` or `Stop`).
  - `Cmd` (`Play`, `Stop`, `Seek(beat)`, `Loop(beats)`) applies at the next block line.
  - A loop's end frame is computed from its beat every time (RENDER-9a).
  - `retime` keeps the beat across a change of tempo map or rate.
- **The tempo map and arrangement live in the renderer too.** So a tempo change (RENDER-27) is
  served today as a new renderer that `enter_from`s the old transport, retimed.

### 3.2 Messages and their order (engine.rs:30-60, 342-463)

- `Msg` is one of:
  - `Activate{ renderer: Box<Renderer> }`;
  - `SwapLane{ generation, slot, data: LaneSwap }`;
  - `Transport(Cmd)`;
  - `Panic`.
- **Transport and Panic apply as soon as they are read.**
- **Structural messages apply in strict order.** One that can't apply yet waits in `head`, and the
  next in `blocked`. Only consecutive Activates coalesce: the newer one retires the one that never
  played.
- `Retired` is `Renderer(Box<Renderer>)` or `Lane(LaneSwap)`. Nothing that owns memory is dropped
  on the audio thread:
  - the retire callback sends it to the player's `bounded(64)` channel;
  - when that is full, the engine keeps it in a 16-slot `stash`;
  - the callback keeps 8 more (`gone`);
  - when all of those are full, it is leaked with `mem::forget`, never freed there.

  RENDER-25's *Today* text about an "8-slot retired queue … dropped on the audio thread" is stale.
- **Stamps:** the last 16 `Applied` (when a message took effect) and `Heard` (when cell writes were
  applied) are published through the player's seqlock.

### 3.3 Activation and the 5 ms crossfade

`try_apply(Activate)` (engine.rs:342) works like this:

1. **It waits while a fade runs.** `ready = current.is_none() || fading.is_none()`.
2. `renderer.enter_from(old.transport, old.tempo, rate)` copies the transport, retimes it and sets
   `Enter`. Nothing is chased: the new schedulers seek their cursors to the position and start
   empty.
3. `renderer.carry_from(&mut old)` hands over the plugin instances the two renderers share
   (section 6).
4. The old renderer becomes `Fade{ old, done: 0, first: true }`.

`render` (engine.rs:465) then does this:

- The current renderer renders first.
- The old one renders with `release_all = first`, which sends every note an Off at frame 0, and
  with `lend`: each lent plugin node copies the new node's output, so a shared instance is
  processed once.
- The two outputs are mixed linearly over `fade_len = round(0.005 × rate)`, 240 frames at 48 kHz,
  so **both renderers run whole for 2 blocks**.
- The old renderer is retired at the next block line once the fade is done (or stashed, as in
  3.2).

**What this costs:** about 2× the song's DSP for 2 blocks on every structural edit. p50 at step 8's
70% budget would overrun the block (*estimate*).

### 3.4 SwapLane, jumps, panic, idle

- **`SwapLane`** reaches only the renderer with that `generation`. The scheduler swaps its data
  (refused when it doesn't fit, `refused_swaps`) or the control lane's `Arc` is replaced. The
  replaced data is retired.
- **Jumps** (seek, wrap, stop, start) are transport runs. Schedulers end their sounding notes and
  audio nodes crossfade; kernels and plugins keep their state (`a_jump_never_resets_a_kernel`,
  `a_loop_keeps_its_tails_for_10_minutes`).
- **`Panic`** resets the whole graph in place: `k_reset` on every kernel, plugin `reset`, feedback
  rings cleared, voices freed, schedulers reset. The next block is `Enter`.
- **Idle** (RENDER-6a): the engine idles once it is stopped, after 8 silent blocks, with no fade
  running, when `can_idle` holds. A message, a cell write or a plugin's `request_process` wakes it.
- **A trap or a NaN** in strict mode is an error. Live mode silences the renderer: `broken`, then
  `Stop`, and `crashes` is counted.

### 3.5 Player and callback (`core/play/src/lib.rs`)

- `Live { engine: Engine, resampler: Option<Resampler> }` runs the engine at the project's rate and
  converts at the device boundary (RENDER-31).
- `Callback` holds `live`, `fade: Option<LiveFade>`, `held`, `gone[8]`, `rx` (`bounded(1024)`),
  `retired` (`bounded(64)`) and two 8192-frame scratch buffers.
- **Player messages:**
  - `Engine(Envelope)`;
  - `Replace{ live, keep }`, used by `Player::open` (a new project) and `Player::replace` (a rate
    change);
  - `Unload`.

  `Replace` and `Unload` swap the whole `Live` with **a second crossfade mechanism**: 5 ms at the
  device's rate (`LiveFade`).
- **`Player::send` retries `try_send` for up to 500 ms,** then reports `Stalled` and drops the
  message, telling the caller. RENDER-28 says "never dropped … the sender waits"; this is the
  bounded form of that.
- **The `m2-play-drop` thread** polls the retired channel every 5 ms and drops what it finds. It
  bears every renderer's teardown: dropping thousands of instances, unmapping each one's memory, and
  queueing plugin destroys to the main thread.
- A panic inside the engine is caught at the callback (`catch_unwind`), and the callback plays
  silence until a project is opened again.

### 3.6 The allocation trap and other RENDER-28 guards

- **`core/play/tests/no_alloc.rs`** runs the callback on a fresh thread under a counting global
  allocator. The run plays, seeks, loops, steals voices, swaps a lane, activates across a tempo
  change (which crossfades and carries two probe plugins), stops, idles, panics, fills the retire
  channel, and writes cells from another thread throughout. It asserts no allocation, and
  `kernel_memory_bytes` unchanged (memory growth inside wasm is mmap or mprotect, which the
  allocator never sees).
- **Grep tests:** `core/play/tests/no_blocking.rs` checks the player's channels never block, and
  `core/plugins/tests/no_blocking.rs` checks the host's `audio.rs` has no locks, allocation or
  thread-locals.
- **What isn't built:** the contract's "debug builds … run the audio thread under an allocation and
  lock trap" exists only in tests. The app's own debug build has no trap.

---

## 4. Offline, range render and export

- **`Renderer::new`** builds with `build_live(…, Owner::discard(), &mut Fresh)`. It plays from 0
  with the transport equal to the engine clock and is strict.
- **`Renderer::seek(to)`** resets, rewinds the transport to 0 and replays to `to`. This is RENDER-1's
  definition, offline only.
- **`Render::range`** uses `starting_at(t0)`: a fresh build with `Transport::starting(t0)`, which
  is jump semantics. It is approximate unless `t0 == 0`.
- **Export Audio** (`export::start`): the actor builds a `Render`, which means **a second complete
  set of kernel instances and a second set of plugin instances**, created with `Fresh`. The render
  runs on its own thread beside playback. Nothing is paused, `set_render_mode(Offline)` is never
  called, and the plugins' state comes from the blobs as committed.
- `m2 render`, `m2 bounce` and `m2 plugin-state` (`apps/m2`) use `Renderer::new` or the host
  directly.
- Tests:
  - `seeking_gives_the_same_samples_as_a_full_render`;
  - `every_buffer_size_gives_the_same_samples`;
  - `poly_plays_notes_from_the_timeline_and_is_deterministic`;
  - the RENDER-33 test in `core/play/tests/bit_identity.rs`: live equals offline on the 4 pure
    fixtures, plus a no-op swap test.

---

## 5. The kernel ABI (`core/kernel/src/lib.rs`) and package loading

- **ABI v0.** A module exports `m2_abi() -> 0`. For each kernel `k` it exports:
  - `k_init(sample_rate: f32, max_block: u32, channels: u32)`;
  - `k_ports() -> inputs<<8 | outputs`;
  - `k_port(i) -> ptr`;
  - `k_reset()`;
  - `k_process(frames)`;
  - `k_active() -> u32`.
- **No imports.** State is the instance's memory.
- **Each kit kernel lives in a `static mut HOST: Option<Host<K>>`.** A `Host` keeps
  `PORT_CHANNELS(2) × max_block` floats per port. Reset is `K::new` again, with the outputs zeroed
  and the inputs left alone (ABI-1).
- **There is no state save or load and no latency or tail query** (both reserved in ABI-2). So the
  only ways to carry a kernel's state are to keep its instance, or to copy its whole linear memory
  into an identically shaped instance (same module bytes).
- **Package loading** (`core/runtime/src/packages.rs`):
  - `PackageSet::load` reads `m2pkg.toml`, compiles the package's **one** wasm module
    (`Module::new`) and verifies it: no imports, ABI v0, every declared kernel's six exports with
    the right signatures, checked by instantiating once.
  - It records `Package.files[path] = sha256` for provenance.
  - `module_for(kernel_id)` returns `(module, export prefix)`.
- **Hashes.** A kernel's "content hash of its wasm module" is today the package's `std.wasm` hash:
  every std kernel shares it, so rebuilding `std.wasm` changes the shape of every std instance.
  This errs on the safe side.
- **Packages load once per session** (`packages()` in actor.rs:282, and `Session::open`). A wasm
  rebuilt on disk never reaches a running session. Step 7's gate "a kernel module rebuilt with the
  same kernel id but a new hash starts from init" needs either a package reload path or a
  runtime-level test with two module builds (the simplest).

---

## 6. PluginHost and carried instances

### 6.1 The host (`core/plugins`)

- **The `PluginHost` trait** is in `host.rs`, as built in step 3b. It has `create` (init, state load,
  ports, params), `activate` (which returns the latency), `deactivate`, `load_state` (skipped when
  the hash equals the state last saved), `set_render_mode`, `main_call`, `destroy` (queued),
  `latency`, `save_state`, `process`, `reset`, `take_process_request` and `live`.
- **`InProcess`** (`inprocess.rs`):
  - a fixed slab of `MAX_INSTANCES = 1024` slots, each a generation plus an `AtomicPtr<Core>`, so
    the audio thread finds an instance with two atomic loads;
  - every main-thread call runs on the plugin main thread (HOST-2), with libraries loaded on the
    calling thread;
  - `activate` sizes the audio side's buffers (so `process` never allocates) and reads the latency.
- **What the runtime calls:** `create`, `activate`, `process`, `reset`, `take_process_request` and
  `destroy` (by drop).
- **What it never calls:** `deactivate`, `load_state`, `set_render_mode` and `save_state`. Only
  tests, `crates/gates` and `apps/m2` call the last of these.

### 6.2 Carrying across rebuilds (`core/runtime/src/plugins.rs`, HOST-5)

- **The key.** `CarryKey { module, plugin id, version, state_path: Option<Vec<key>>, ordinal,
  state: Option<Sha> }`. The `ordinal` counts top-level nodes with the same
  `(module, plugin, state_path)` in Flat order.
- **`PluginInstance`** holds `host`, `h`, `key`, `rate`, `restart`, `held_cap` and `next_id`. Its
  `Drop` queues `host.destroy`.
- **The source of instances.**
  - `Carry::of(renderer, epoch)` pools the top-level instances of the renderer last sent. The actor
    keeps it in `Open.carry`.
  - `Carry::source(epoch)` gives the pool to a build for the same engine epoch and rate, and `Fresh`
    otherwise.
  - `take(key, rate)` matches the first instance that isn't taken, has an equal key and didn't ask
    for a restart.
- **At activation, on the audio thread** (`carry_from`, plugins.rs:193):
  - For each new plugin node whose instance an old node holds, the new lane scheduler `adopt`s the
    old scheduler's sounding notes, with their ids, when both read the same lane. It also raises its
    `next_id` above every id the instance has seen.
  - Notes the new scheduler holds move to the new node's `keys`. The rest go to `release`, to get a
    NoteOff at the first call.
  - The old node is `lent`: for the fade, it copies the new node's output (`lend`).
- **Not carried:**
  - an instance that asked for a restart;
  - a changed state blob, which means a new instance with the new blob, not `LoadState`;
  - offline renders, and a new engine (`Replace`, `Load`);
  - plugins inside voices: the runtime handles them (`ordinal = u32::MAX`), but the **checker
    already refuses** plugins in `poly` (check.rs, "plugins can't be inside poly"). That branch is
    dead code.
- **A real defect of ordinal matching.** Take two unkeyed instances of one plugin id in one module.
  Inserting a third before them shifts the ordinals, so the first old instance (with its state and
  tails) goes to the newly inserted node. ID-13 forbids exactly this kind of guessing. A RuntimeId
  from frames removes it.
- Tests are in `core/runtime/src/tests/live/carry.rs`:
  - `a_carried_instance_keeps_its_notes`;
  - `notes_the_edit_removed_are_let_go`;
  - `ids_never_repeat_after_a_renderer_without_notes`;
  - `keyed_instances_follow_their_keys`;
  - `a_changed_state_is_a_new_instance…`;
  - and others.

  The gates have `crates/gates/src/plugins.rs` (with-plugins, G5 and G7).

---

## 7. What each kind of change does today

| Change | Path (actor.rs @HEAD) | What is rebuilt | What it costs | State lost |
|---|---|---|---|---|
| Knob preview or lease update | `Player::set_cells` to the playing renderer's slots | nothing | a few atomics | none |
| Committed value, same cell layout | `Reach` with cell writes | nothing | a delta walk plus `value_writes` (clones and compares both Flats) | none |
| Committed value whose layout differs | rebuild | everything | as a structural edit | as a structural edit |
| Notes or control lane edit that fits | `Reach` with `SwapLane` per lane | the lane's `NotesData` or `ControlLane`, off the audio thread | `NotesData::build` (a sort); G11 measured 26.6 ms p95 from commit to first block in p50 | none (RENDER-26a reconciles) |
| Notes edit beyond `Caps` | rebuild | everything | as a structural edit | as a structural edit |
| Audio lane, asset, plugin state, arrangement | rebuild | everything; clip renders from the cache (a tempo change re-renders every clip) | as a structural edit, plus clip renders | as a structural edit; a changed state also means a new plugin instance |
| Code (structural) | rebuild | everything | section 8 | section 9 |
| Manifest `sample_rate` | `Way::Replace`: a new engine, `Fresh` | everything, plugins included | as a structural edit, plus every plugin created | everything except the beat, play state and loop |
| A new player (stall) | `Way::Load`, `Fresh` | everything, plugins included | as above | everything except the position |
| Seek, loop, stop, play | transport `Cmd` | nothing | none | sounding notes end (RENDER-9); tails ring |
| Panic | `Msg::Panic` | nothing; everything is reset in place | `k_reset` on every top-level kernel and every voice that has played, up to about 6,100, on the audio thread (each heap already grown by prewarm) | everything (RENDER-10) |
| Export, render, bounce | `Renderer::new` on the actor; renders on its own thread | a second full renderer, plugins included | as a structural edit, plus all plugins | none live (it's separate) |

---

## 8. A structural edit at p50, step by step

Before the 200 ms budget starts: the compile (QuickJS build plus check), 44 ms p95 and up to 84 ms
(PLANE-4).

After the compile, on the actor thread:

1. **`delta_between(plays.revision, accepted)`:** a line diff of each changed code file, then both
   IRs walked and named. Its result is used only as a yes or no on "structural".
2. **`Renderer::build_live`:**
   1. `Store::new`, with the limiter.
   2. `lane_slots`, then each `NotesData::build` (all of p50's notes, sorted per lane), each
      `data.control()` (a clone), and each `AudioLane::build` through the cache.
   3. `Builder::graph`: for each of about 6,100 kernel nodes, `Instance::new` (one mmap reservation
      of 4 GiB plus guards, a vmctx malloc, a copy of about 9.8 KB of data segments), six
      `get_typed_func` lookups by formatted names, `k_init` (the kit's `Host::new` allocates port
      buffers and state inside wasm), `k_ports`, `k_port` × ports, an overlap sort, const fills, and
      `Vec` buffers per output port.
   4. Plugins: `Carry::take`, or `host.create` plus `activate` on the plugin main thread. The old
      renderer plays meanwhile, and plugin loads don't count against the budget.
   5. `prewarm`: `k_reset` on every kernel, which runs `K::new` again (a delay re-allocates 2 s
      buffers).
   6. `Cells::new`: `cells(flat)`, `CellMap::of`, and `CellSlots` for every cell.
3. `audio.prune()`, then `describe` and `Carry::of`.
4. **Phase 1 under the control lock:** each kept lease's value is set into the new cells, then
   `Player::activate`, which sends the message.

On the audio thread, at the next block line:

5. `enter_from`, `carry_from`, then the fade: two blocks at about 2× DSP.
6. Old renderer retired → `m2-play-drop` drops a `Store` of about 6,100 instances (one munmap
   each), its lane data and its cells, and queues destroys for plugins nobody holds any more.

**Measured and estimated:**

- On the reference project: an added effect was audible in about 54 ms after the compile (step 3a
  commit `f1a3a2e`). The message says "p16/p50"; the gates default to p16, which has about 1,890
  kernel instances by the count in 2.4. That time includes the build, queueing, at most a device
  buffer, and the fade.
- p50, by scaling (×3.2 instances): about 150 to 170 ms for the build alone, if the build is most of
  the 54 ms (*estimate*). The cost of instantiating and tearing down per instance is unmeasured. The
  with-plugins G5 gate prints the plugin-inclusive build time but doesn't keep it.
- **The 200 ms gate may pass today by luck. The tails gate can't.**

---

## 9. State inventory: what survives what, today

| State | Where | Seek, loop, stop | SwapLane | Activation (structural) | Panic |
|---|---|---|---|---|---|
| Kernel memory (phases, filters, envelopes, delay and reverb lines) | wasm memory in the renderer's `Store` | kept | kept | **lost: new instances from init** | reset |
| Voice assignment, gates, `born` | `PolyNode`, `VoiceSlot` | notes end, releases ring | kept | **lost** | reset |
| Sounding notes and their ids | `NoteScheduler.sounding`, `next_id` | Offs sent | reconciled | **Offs sent, faded in 5 ms; not chased**, except notes adopted by carried plugins | cleared |
| Feedback ring | `Exec::Feedback.ring` | kept | kept | **lost** | cleared |
| Audio lane taps and fades | `AudioNode` | fade across the jump | kept | **lost** (whole-output fade instead) | reset |
| Clip renders, decoded assets | `audio::Cache` (`Arc`) | kept | n/a | **kept** if the key matches | kept |
| Cell values | `Cells.table`, const bufs, kernel inputs | kept | kept | taken from the new Flat, plus live leases set at build; a ramp in progress is lost | kept |
| Cell slots, `CellMap`, `CellId`s | `Arc`s per renderer | kept | kept | **replaced**; leases aimed again | kept |
| Plugin instance (state, voices, tails) | `PluginInstance` via the host | kept | kept | **kept if `CarryKey` matches**; otherwise new | reset |
| Transport (position, play state, loop) | `Renderer.transport` | moves | kept | **copied and retimed** | `Enter` |
| Engine clock | `Engine.clock` | kept | kept | kept | kept |
| Lane slot numbers, `generation` | per renderer | kept | kept | **replaced**; in-flight swaps to the old generation are dropped | kept |

---

## 10. Against step 7's gates and rules

| Gate or rule | Today | What stands in the way |
|---|---|---|
| A structural edit at 3:00 in p50 is audible within 200 ms of the compile | 150 to 170 ms build (*estimate*), plus queueing and the device | a full rebuild of about 6,100 instances; the build is O(song), not O(edit) |
| …and untouched tracks' tails are intact | **no**: every kernel is new, every note is cut | one `Store` per renderer (2.3), no node identity in `Flat` (2.1), schedulers per renderer (2.5) |
| Live playback with no edits equals offline | **yes** (`bit_identity.rs`) | keep it: the arena must not change the per-block order or the sums |
| A rebuilt kernel module with the same id and a new hash starts from init | not reachable: packages load once per session | needs a shape (module hash) in the arena, and a reload path or a runtime-level test |
| Export of with-plugins pauses playback, reuses instances, and resumes with state intact | **no**: export creates its own instances and runs beside playback | `load_state`, `set_render_mode` and `deactivate` exist but nothing calls them; no pause protocol between the engine and the export |
| Store cost at 6,176 instances measured, layout chosen | not measured | section 12 |
| RENDER-25 (keep matching instances) | plugins only, by `CarryKey`; kernels never | as above |
| RENDER-25a (fade only the changed subgraph) | whole output, 2× CPU | `lend` and two renderers |
| RENDER-26b (`LoadState`) | a changed blob means a new instance | `CarryKey.state` |
| RENDER-27 (`SetArrangement` prepared off the audio thread) | a tempo change means a new renderer, retimed | `TempoMap` and `NotesData` frames baked into the renderer |
| RENDER-1 (export reuses live instances) | no | as above |

---

## 11. What step 7 can delete or unify, and contract edits to propose

These follow from the map. The other step 7 notes do the design itself.

1. **Delete `Carry`, `CarryKey`, `InstanceSource`, `Fresh`, the ordinal, `carry_from`, `lend`,
   `PluginNode.lent` and `release`, `PluginInstance.held_cap` and `next_id`, and
   `NoteScheduler::adopt` and `raise_next_id`.** All of it exists because instances and schedulers
   die with their renderer.
   - An arena keyed by RuntimeId keeps instances.
   - Lanes are already addressed by *saved* keys (`LaneAddr`), so **one scheduler per `LaneAddr`
     for the life of the engine**, with its data swapped, keeps its sounding notes and ids with no
     adoption.
   - A consumer that an activation retires is told to let go of its own notes (RENDER-26a bullet 2).
     That moves `release_all` from the schedulers to the retiring consumers.
   - It also fixes the ordinal hand-off defect (6.2). HOST-5's *Today* paragraph goes with it.
2. **Delete the whole-renderer crossfade, the two-renderer `Fade` in `Engine`, and `release_all`.**
   RENDER-25a already requires fading only at the outputs of replaced subgraphs from step 7. Keep
   `LiveFade` in `core/play` for the engine-replacing cases: a new project, a rate change, a close.
3. **Delete `same_layout` and `value_writes` in the server (actor.rs:151-211).** Once cells are
   addressed through the active plan's `CellMap` from ControlIds (RENDER-23), a value edit is a cell
   write whatever the layout. The "values without `same_layout` → new renderer" row of section 7
   disappears.
4. **`SwapLane` by `LaneAddr`, not `(generation, slot)`.** The contract's sketch (RENDER-32's engine
   shape) already says `SwapLane(LaneAddr, …)`. `lane_slots` numbering and the generation guard can
   go.
5. **Delete the runtime's plugin-in-voice branch** (`in_voice` in `Builder::plugin`): the checker
   refuses it.
6. **ID-13 and ID-15.** Step 6's delta already names every node by `[module, frame chain (key:k, or
   the call's range in shared coordinates plus its occurrence), op and call place, #occurrence]`,
   between *any* two revisions (`delta_between`, using `Edits::between`, a Myers diff over lines
   trimmed to bytes, and a Tx's own `Coords`).
   - That *is* a correspondence between the plan that plays and the new one. Pairing old and new
     Flat nodes by delta name gives the arena its matching with no persistent `SiteId(u32)`
     allocator.
   - Proposed edit to ID-13: say that the arena matches by the delta's node correspondence from the
     plan that plays to the new one.
   - Proposed edit to rule 3: say "a Myers diff over lines, trimmed to the changed bytes", which is
     what's built, rather than "token-level".
   - Proposed edit to rule 4 (a unique structural match): move it to Deferred, next to "keyed
     iteration helpers and better structural matching".
   - The cost of this approach is step 6's risk 1 (a reflowed track call renames its nodes, so that
     track resets). A producer would hear it only on hand-reformatting, and only on that track.
7. **RENDER-32.** Per-instance versus per-partition `Store` is the wrong axis for cost.
   - The count of instances and memories (one 4 GiB reservation each, data copied, vmctx) is the
     same either way. What a per-instance `Store` adds is one `StoreOpaque` per instance.
   - The deciding facts are that a `Store` never frees instances (so a long-lived shared store
     leaks every replaced one) and that movable instances need their own store.
   - Proposed edit: "one `Store` per kernel instance; measure create, RSS, address space and call
     overhead at 6,176; if instantiation dominates, use wasmtime's pooling allocator or a smaller
     `memory_reservation`, not a shared store".
8. **One wasm module per package means one shape hash per package** (section 5). That's fine, and
   safe, until per-kernel modules exist. Say so in RENDER-25's shape definition, so the gate's
   "rebuilt with the same kernel id" is read as "the package's module bytes changed".
9. **RENDER-28.** Record that `Player::send` reports `Stalled` and drops after 500 ms. The contract
   says the sender waits or coalesces. Also update RENDER-25's stale "8-slot retired queue … dropped
   on the audio thread" *Today* text: nothing is ever dropped there now.

---

## 12. Measurements to take first (none were possible here)

1. **`Renderer::build_live` on p50, split into parts:** `Instance::new`, typed-func lookups, `k_init`,
   prewarm, `NotesData`, `Cells`. Use release builds on the reference M1.
2. **Per instance at 6,176:** `Store::new` plus `Instance::new` time, RSS (`task_info`) and address
   space. Do this with one shared store and with one store per instance; also with `memory_reservation`
   at 4 GiB versus, say, 1 MiB with `memory_reservation_for_growth`; and also with the pooling
   allocator.
3. **The per-block call overhead** of `k_process` across 6,176 stores against one store (cache
   effects).
4. **Teardown time** on `m2-play-drop` for one renderer at p50. Check whether munmap storms
   contend with the audio thread through the VM map lock.
5. **The CPU of today's two-renderer fade at p50:** the callback's peak during an activation,
   against its budget.

## 13. Tests that pin today's behaviour (keep or port)

- **RENDER-33:** `core/play/tests/bit_identity.rs` (4 pure fixtures, plus a no-op swap).
- **RENDER-28:** `core/play/tests/no_alloc.rs`, `core/play/tests/no_blocking.rs`,
  `core/plugins/tests/no_blocking.rs`.
- **Jumps and tails:** in `core/runtime/src/tests/live.rs`, `a_jump_never_resets_a_kernel`,
  `every_graph_reset_is_counted`, `activations_crossfade_without_jumping_back`,
  `an_activation_across_a_tempo_change_keeps_the_beat`, `structural_messages_keep_their_order`,
  `a_full_retire_channel_holds_messages_never_drops_them`,
  `activations_while_a_voice_sounds_dont_click`; in `core/play/tests/live.rs`,
  `a_loop_keeps_its_tails_for_10_minutes`, `p16_loops_ten_minutes_without_drift`,
  `no_edit_or_jump_leaves_a_note_stuck`, `random_edits_and_jumps_never_strand_a_gate`.
- **Schedulers:** `core/runtime/src/sched.rs` tests (swaps, ordinals, `release_all`, adoption,
  wraps).
- **Cells:** `core/runtime/src/tests/cell.rs` (step and ramp, voice cells, written once, ABI-1).
- **Carrying:** `core/runtime/src/tests/live/carry.rs`. These go away or are rewritten against the
  arena.
- **Offline:** `core/runtime/src/tests.rs` (determinism, buffer sizes, seek equals a full render).
- **Gates:** `crates/gates`. `--reference 50` runs the timed gates on p50; `plugins.rs` covers
  with-plugins.
