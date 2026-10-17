# 8b back ends: design-8b checked against the code

For the `8b-backend` track (worktree `~/m2-runs/wt/be`, branch `8b-backend`, base `09b2dfd`). It covers 8b.4 (mute,
solo, mute in song), 8b.5 (clips by id, automation drawn in place and removed, markers), 8b.6 (audition) and 8b.8
(meters and the DSP load). It reads `docs/designs/step8/design-8b.md` (written at `cbf5eb9`, before step 7) against the
code as built. Nothing here was built or run. Sizes are rough: S is under 2 hours of agent time, M is half a day, L is
most of a day *estimate*.

## In short

- **Everything in these four features can be built now.** Step 7 is built: `Renderer::prepare`, `PlanInfo`, cells,
  persistent schedulers, engine-wide note ids, fades and ghosts. 8a is **not** built: the runtime renders on one thread.
  So overlays and taps need no partition labels yet. 8a adds them later.
- **What changed since the design, in seven lines:**
  1. Mute in song reaches every offline render through a new `Flat.muted`, not a new argument to `prepare`.
  2. Step 7's fades and ghosts must read through the overlay op. The design didn't cover this. It is the hardest part.
  3. `M2_AUDIO=none` already plays in real time. No "paced" mode is needed for meters.
  4. 8a's load record doesn't exist. The DSP load needs a small callback timer in `core/play`.
  5. The keyed compressor collides with the sounds merge's std 0.2. It waits for it.
  6. Markers live in `layout.json`, whose code (`layout.rs`) is on taste-next. Markers wait for it.
  7. The piano roll's whole-lane route (`/api/graph/lane`) stays until the UI moves to clip gestures.
- **12 slices can run now, in order** (B1 to B12). **Two wait** for other tracks (W1 keyed compressor, W2 markers).
- **To stay clear of taste-next:** new code goes in new files. Shared files get one-line hooks. Never extend the
  `Gestures` trait. Our routes go in their own router file. Never edit `apps/m2/tests/lock.rs`.
- **Tests per slice:** only the crates touched, plus `m2-session` whenever the runtime changes (its renders gate). The
  full check runs once, at the end.

---

## 1. What changed since the design

| # | The design assumed | The code now | What it means here |
|---|---|---|---|
| C1 | 8b.4, 8b.6, 8b.8 wait for step 7 slice 2 | Step 7 is built (`core/runtime/src/plan.rs`, `keep.rs`, `lib.rs` `Renderer::prepare`, `Basis::Keep`, `PlanInfo`, `Continues`) | Nothing waits for step 7 |
| C2 | Overlays and taps get 8a's partition labels (design-8a §3.3); DSP load comes from 8a's `Player::load()` | 8a isn't built. `render_block` calls `graph.process` once, on one thread | No labels now. Taps are read after the graph runs. A small DSP timer goes in `core/play`. 8a must later label overlay nodes and taps |
| C3 | `OverlayInit { values }` passed to `prepare` and `Renderer::new` | `prepare` and `new` are called from many places (server, gates, tests, export's `Renderer::lending`) | Use `Flat.muted` (the checker fills it) for offline renders, and `Renderer::set_overlays(&[f32])` for live ones, called before the plan is sent. No signature change |
| C4 | Fades "see through" overlays because overlays aren't in `Flat` | Fades and ghosts are real exec nodes that rewire consumer inputs (`keep::wire`, `Graph::rewire`, `Edge::{Input, Capture, Feedback, Out}`) | The overlay insert runs after `keep::wire` and must put itself under any fade on the same input. Ghosts that read another track's audio must read through an overlay too. New test (§2.4) |
| C5 | `M2_AUDIO=paced` for the meters suite | `Player::silent()` (`M2_AUDIO=none`) already renders in real time with no device (`core/play/src/lib.rs:774`) | Drop "paced" |
| C6 | The G11 workflow is rewriting `notes.rs` | Done. `notes::write_notes` is final; edits get the PLANE-5 preview path through `Actor::edit` (`previewing`) | Clip gestures get the fast data path for free |
| C7 | `ModuleChange::Muted` is new | It is already in the write API, inert (`core/song/src/tx.rs:199`), parsed, printed, and inverted by undo (`core/session/src/tx/mod.rs:1957`) | 8b.4 doesn't change the write API |
| C8 | The channel lookup walks the graph (§2.3) | taste's 8b.1 (`a0d5f91`) built `crates/editor/src/mix.rs` from the reads' places in the source (`Mixer::channel_of`). No `feeds` | Our `feeds` live in our own code. Mapping a feed in `song` to its channel's track uses mix.rs after the merge |
| C9 | view.json and the panic route are 8b.2's | Both are in taste's WIP 8b.2 (`4804247`: `graph/view_file.rs`, `POST /api/graph/panic`) | We add neither. The runtime makes panic end live notes |
| C10 | `the_write_api_is_step_6s` added in 8b.1 | It is in taste's 8b.1 (`core/song/tests/write_api.rs`), not here | It arrives with the merge. Our slices never change the write API |
| C11 | The sidechain gate needs a keyed compressor in std | sounds-merge is changing `std/src/fx.ts`, `std/kernels-src/src/fx.rs`, `m2pkg.toml` and `std.wasm` (std 0.2) | The keyed compressor waits for std 0.2 on main (W1). The `env` and `follow` fixtures prove two of the three clauses first |
| C12 | Test paths `core/runtime/tests/overlay.rs`, `core/runtime/tests/live.rs` | Runtime tests live in `core/runtime/src/tests/`; `tests/live.rs` there is step 7's live-engine suite | Use `core/runtime/src/tests/overlay.rs`, `audition.rs`, `taps.rs` |
| C13 | Audition names | `m2_editor::controls::Audition` already means a knob preview landing | Call the engine's message `Live`, the events `LiveEv`, the server module `audition.rs` |
| C14 | The server resolves a lane's slot "in the PlanInfo of the plan it last sent" | `PlaysView` (`crates/server/src/graph/lease.rs:369`) holds cell slots and maps, not lanes | `PlaysView` gains the lane refs, the overlay sites and the taps |
| C15 | `the WebSocket` carries audition | `ws_loop` ignores every incoming message (`crates/server/src/routes.rs:167`) | `ws_loop` learns to read `{"t":"audition"}` and reply to that socket only |
| C16 | `client` per window | Built: `/api/ws?client=`, `Graph::client_gone` ends that window's leases | Reuse it for audition sources and their release |

---

## 2. 8b.4 Mute, solo and mute in song

### 2.1 What exists

| Piece | State | Where |
|---|---|---|
| MON-1 to MON-5, the DIAG code `overlays.active` | contract text; *Today*: not built | `docs/architecture.md:1064-1085`, `:928` |
| `ModuleChange::Muted` | in the write API, inert | `core/song/src/tx.rs:199`, undo `core/session/src/tx/mod.rs:1957` |
| What a sidechain input received | built: `Frame.sidechain`, `Flat.sidechain` (sources), `Voice.sidechain` | `core/ir/src/lib.rs:123`, `core/ir/src/check.rs:134`, `:344` |
| std's sidechain ports | `duck`'s `side` (a control), `follow`'s `in` (audio) | `std/src/fx.ts:21`, `std/src/dsp.ts:81` |
| A keyed compressor | missing | `std/src/fx.ts:161` has no `key` |
| Cells, slots, maps, `Player::set_cells` | built | `core/runtime/src/cells.rs`, `core/play/src/lib.rs:953` |
| Inserting a node on an edge | built for fades: `Graph::push`, `Graph::rewire`, `Edge` | `core/runtime/src/lib.rs:1116`, `:1328`, `keep.rs:33` |
| One-block ramp for smooth cells | built; overlay cells not | `cells.rs` `Cells::apply` (`:288`) |
| A cell write without a rebuild | built: `reach_to`, `value_writes`, `same_layout` | `crates/server/src/graph/actor.rs:195-297` |
| Lease values put into a new renderer before it is sent | built (the pattern to copy) | `actor.rs` `settle`, the `r.set_cells(map.cells(&c.origin), …)` loop |
| The driven player for the click test | built: `Player::driven` | `core/play/src/lib.rs:798` |
| A track-wrapped sine for the gates | missing (`songs::sine` has no track) | `crates/gates/src/songs.rs:96` |
| `.music2/overlays.json` | a known Local path; nothing reads or writes it | `core/song/src/data/tests.rs:83` |
| `track()` options | only `name` | `core/script/sdk/core.ts:1634`, `:1658` |
| `Module.muted` | missing | `core/ir/src/lib.rs:53` |
| The mixer lookup, strips | on taste (`mix.rs`), not here | — |

### 2.2 What the back end needs

**IR and checker** (`core/ir`):
- `Module.muted: bool`, `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`. Every IR without it keeps its
  JSON and hash (ID-11). The checker refuses `muted` on the root module.
- `Flat.overlays: Vec<OverlaySite>`:
  ```rust
  pub struct OverlaySite { pub module: String, pub consumer: Consumer, pub channels: u8 }
  pub enum Consumer { Input { node: usize, input: u16 }, Capture { poly: usize, index: u16 }, Out }
  ```
  A site is an input (or a voice capture) of a reached node of module `c` that reads an **audio** output of a node of
  track `m ≠ c`, and isn't a sidechain input. `Out` when `Flat.out` reads a track's node. Same code, same list.
- `Flat.side_inputs: Vec<(usize, u16)>`: the inputs, inside a frame that recorded a sidechain, that read the recorded
  signal. FNode has no frame, so compute it in `check()` from the IR nodes' `frame` chains and `slot_of`.
- `Flat.muted: Vec<String>`: the track keys with `muted`, sorted.
- Fix the few `Flat { … }` literals (`core/session/src/lib.rs:88`, `check.rs:512`).

**SDK** (`core/script/sdk/core.ts`): `TrackOptions.muted?: boolean`; `options(…, { name: "string", muted: "boolean" })`;
the module JSON gets `muted: true` only when true. `SDK_VERSION` 1.1.0 → 1.2.0 (`core/script/src/lib.rs:47`, FMT-2).

**Delta** (`core/session/src/tx/delta.rs`): produce `ModuleChange::Muted` for a module in both sides whose `muted`
differs (B1). `structural()` (`:86`) ignores `Muted` from B3 on, once the server turns it into cell writes; until
then a toggle is an activation, which is correct, only slower.

**Runtime** (`core/runtime`):
- `Exec::Overlay { src: Input, overlay: u32, target: f32, cur: f32, step: f32 }`. Per block: at `cur == target == 1`
  it copies (bit-exact); otherwise `cur` moves toward `target` by `step` per frame, clamped, and each sample is
  `x × cur`. `step = 1 / round(0.010 × rate)`.
- New `core/runtime/src/overlay.rs`: `place(flat, &mut graph, &mut wiring)`, run in `Renderer::build` after
  `keep::wire` and the fade pushes. For each site: push the node, put it in `order` and `order_after` just before the
  consumer, and rewire the consumer's input to it. If `w.rewire` already points that input at a fade, rewire the
  fade's `new` side instead. A ghost input that reads another track's audio reads through an overlay that shares the
  cell of the new plan's site with the same (track, consumer module), or a fixed value from the init.
- Cells: `CellSlots::new(value_cells + sites)`; overlay `CellId`s come after every value cell, so value ids don't move.
  `Cells::apply` sends an overlay id to its node's `target` (the op glides; no host ramp).
- `PlanInfo.overlays: Vec<(String /*track*/, String /*consumer module*/, CellId)>`.
- `Renderer::set_overlays(&mut self, values: &[f32])`: sets `target`, `cur` and the slot together, before the
  renderer is sent, so a new plan starts where the old one was (no glide at entry).
- Offline (`Renderer::new`, `lending`, `starting_at`): each site starts at 0 when its track is in `Flat.muted`, else 1.
- Overlay nodes hold no box: nothing in `keep.rs`'s moves changes.

**Server** (`crates/server`):
- New `crates/server/src/graph/overlay.rs`:
  - `Overlays { mute: BTreeSet<String>, solo: BTreeSet<String> }` in `ControlPlane` (one field in `graph.rs`);
  - `values(flat, &Overlays) -> Vec<f32>`: design §6.4's path rule over the site graph, mute (and mute in song) wins;
  - `.music2/overlays.json` read at open and written on each change (temp file in `.music2/`, then rename); a
    read-only window keeps them in memory only;
  - `impl Graph { pub fn overlay(&self, token, change) -> Result<Value>; pub fn forget_overlays(&self, keys) }`;
  - the state's `overlays` value (§2.3).
- `actor.rs`:
  - every new renderer gets `set_overlays` before it is sent (beside the lease loop);
  - `reach_to`: a delta whose only structural part is `Muted` is reached by cell writes; `value_writes`' `bare`
    clears `Flat.muted`;
  - a `Rekeyed` change renames the overlay entries (and back on undo).
- `PlaysView` gains the plan's overlay sites, so `overlay()` writes cells under the control lock without the actor.
- Export: `Graph::export` adds the Warning `overlays.active` to `problems` when any site's value differs from its
  mute-in-song value.

**Editor** (`crates/editor`), new file `mute.rs`:
- `pub fn mute_in_song(s: &mut Session, modules: &[String], muted: bool) -> Result<Edited, EditError>`: edits each
  `track()` call's options as `rename_label` edits `name` (reuse `rename::track_call`). One Tx, `Confined { each
  options range or insertion point, modules: [Muted{…}] }`. Labels from design §2.4.
- `pub fn feeds(rev: &Revision) -> BTreeMap<String, Vec<Feed>>`, `Feed { module, label }`: each place a track's
  export reaches another module through a sidechain input or a control export. A fed node in `song` reports `song`
  until mix.rs is here (Q6).

**CLI** (`apps/m2`): `m2 mute-in-song <project> <track>… on|off` (takes the lock).

### 2.3 What the UI will need later

| Kind | What |
|---|---|
| route | `POST /api/graph/overlay` `{session, mute?: {k: bool}, solo?: {k: bool}, exclusive?: k, clear?: true}` → the overlay state. **Not** in WRITES: works read-only, in memory |
| route | `POST /api/graph/track/mute` `{session, revision, modules: [k], muted}` → `{file, before, after, revision}`. In WRITES. Used by the header menu and the export dialog's "Mute Kick in the song, then export" (one Tx) |
| state | `state.overlays = { mute: [k], solo: [k], muted_in_song: [k], heard: {k: "heard" \| "muted" \| "muted-in-song" \| "silenced"}, feeds: {k: [{module, label}]}, saved: bool }` |
| event | the usual `{"t":"graph"}` after a change; no new event type |
| export | the done event's `problems` carries `overlays.active`; the dialog's "don't ask again" lives in taste-next's `view.json` (`export.ask`) |
| refusals | the route refuses `song` (MON-1) in words |

### 2.4 Tests

| Where | Tests |
|---|---|
| `core/script/src/tests.rs` | `muted_is_a_track_option`, `an_unknown_option_is_still_refused` |
| `core/ir/src/check.rs` (tests) | `a_song_without_muted_has_the_same_ir_hash`, `the_songs_output_reading_a_track_is_a_site`, `no_site_on_a_sidechain_input` (`follow(kick.out)` read twice), `a_control_export_is_no_site` (`kick.env`), `a_voice_capture_is_a_site`, `muted_on_song_is_refused` |
| `core/session/src/tx/delta_tests.rs` | `muted_is_reported_and_not_structural` |
| `core/runtime/src/tests/overlay.rs` (new) | `an_overlay_at_unity_changes_no_bit`, `an_overlay_glides_over_ten_milliseconds`, `muted_in_song_silences_an_offline_render`, `a_new_plan_starts_at_its_overlay_values` (no glide at entry), **`an_edit_while_muted_keeps_the_track_silent`** (an activation that replaces the consumer, and one that fades its input: the muted track stays out through the window; new, from C4) |
| `core/session` | `fixture_renders_change_only_with_stds_version` (renders.json unchanged) |
| `core/play/tests/no_alloc.rs` | overlay cell writes while playing |
| `core/play/tests/mute.rs` (new) | `toggling_mute_makes_no_step_larger_than_twice_the_sines` at 440 Hz (the gate) and 50 Hz, buffers 128, 37 and 512, toggles mid-block; `a_monitoring_mute_live_equals_mute_in_song_offline` |
| `crates/server/src/graph/tests.rs` | `overlays_persist_in_music2`, `a_read_only_window_keeps_overlays_in_memory`, `solo_keeps_a_bus_fed_and_its_sources`, `mute_wins_over_solo`, `a_track_muted_in_song_stays_silent_when_soloed`, `the_song_module_cant_be_muted`, `a_new_plan_takes_the_overlays_before_it_is_sent`, `a_rekeyed_track_keeps_its_mute`, `a_mute_in_song_toggle_is_a_cell_write` (no `Activate`), `an_export_is_byte_identical_with_overlays_active` (and only that export carries `overlays.active`) |
| `crates/editor/src/mute_tests.rs` (new) | `mute_in_song_inserts_and_removes_the_option`, `several_tracks_mute_in_one_tx`, `undo_puts_the_options_back`, `feeds_name_the_duck` |
| `apps/m2/tests/mute.rs` (new) | `m2 mute-in-song kick on` writes the option; refused while another process holds the lock |
| `core/session/tests/mute_sidechain.rs` (new) | per fixture `env`, `follow` (and `keyed` in W1): `muting_the_kick_keeps_the_bass_ducked` (A′ = kick channel at −96 dB, B = kick muted in song: bit-identical), `without_the_sidechain_the_bass_changes` (negative control) |

### 2.5 Contract edits

MON-1 (sites from the checker, `Out` included; the op is built-in and in every plan, offline too; ×1.0 when heard;
one cell per site; fades and ghosts read through it), MON-2 ("the mixer shows this": `feeds` in the state), MON-3 (the
path rule; mute, in song too, wins over solo), MON-4 (the file's format; read-only windows keep them in memory; the
export warning), MON-5 *Today*, RENDER-22a (overlay cells glide 10 ms in the op; no host ramp; the 50 Hz arithmetic),
RENDER-23 *Today*, PKG-13 (`Flat.side_inputs`), ID-11 *Today* (`Module.muted`), WRITE-8 (`muted` → overlay cells; not
structural), WRITE-10 (mute in song's row), WRITE-18 (`mute-in-song`), the Planes table's control row (overlays),
`docs/gaps.md` #4 (mute and solo).

---

## 3. 8b.5 Clips by id, automation in place, markers

### 3.1 What exists

| Piece | State | Where |
|---|---|---|
| The roll's whole-lane write, by revision | built (WRITE-12's exception) | `crates/editor/src/notes.rs:43` `write_notes`; route `/api/graph/lane`; web `web/src/lib/graph.ts:501` |
| Ids for clips without one, canonical order | built | `notes.rs:150` `tidy` (`ClipId::derive`) |
| Undo words for a lane | built for notes clips | `notes.rs:188` `place`, `:195` `describe` |
| The `Clips` op, canonical lines, `addressable` | built | `core/song` (`LaneData::lines_of`, `clips_with`) |
| The fast data path (heard before durable) | built | `Actor::edit` → `previewing` (PLANE-5) |
| Clip ids in the timeline | built; no hash per clip | `crates/editor/src/timeline.rs:109` `lane` |
| Automate a knob | built; names the lane by binding (`out-decay`, D6) | `crates/editor/src/automate.rs:39` `name_of`, `:119` `automate` |
| Remove automation | missing | — |
| Which lane drives an input, in the view | partly: lane nodes and edges are in the view | `crates/editor/src/view.rs:644-654` |
| `layout.json`, markers | missing here; taste's `layout.rs` keeps `markers` raw ("8b.5 reads them") | taste `a0d5f91` |
| Gesture types, `clips.rs` | missing | — |

### 3.2 What the back end needs

**Editor**, new `crates/editor/src/clips.rs` and `clips_tests.rs`:
```rust
pub struct ClipRef { pub lane: LaneAddr, pub id: Option<String>, pub at: Option<(f64, u32)> } // at: an anonymous clip's start and rank
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Gesture {
    Create { lane: LaneAddr, start: f64, length: f64 },
    Move { clips: Vec<ClipRef>, by: f64, to: Option<LaneAddr>, copy: bool },
    Resize { clip: ClipRef, start: f64, length: f64 },
    Split { clips: Vec<ClipRef>, at: f64 },
    Join { clips: Vec<ClipRef> },
    Duplicate { clips: Vec<ClipRef>, range: Option<(f64, f64)> },
    Paste { clips: Vec<Copied>, lane: Option<LaneAddr>, at: f64 },
    Delete { clips: Vec<ClipRef> },
    Notes { clip: ClipRef, notes: Vec<Note>, length: Option<f64> },
    Points { clip: ClipRef, points: Vec<(f64, f64)> },
    Quantize { clips: Vec<ClipRef>, notes: Option<Vec<u32>>, grid: f64, ends: bool },
    Set { lane: LaneAddr, put: Vec<serde_json::Value>, delete: Vec<String> },
}
/// Pure: what the gesture makes of the lanes it touches (8b.7's lane lease calls it too).
pub fn apply(rev: &Revision, g: &Gesture, mint: &mut dyn FnMut(&LaneAddr) -> String) -> Result<Vec<LaneEdit>, EditError>;
/// One Tx: a `Clips` op per lane touched (a `Write` with base `Absent` for a lane with no file, or the whole lane
/// when it isn't addressable), `Expect::Confined { lanes }`, and the planner's assertion.
pub fn edit(s: &mut Session, g: &Gesture, seen: &BTreeMap<String, String>) -> Result<ClipsEdited, EditError>;
pub fn sha(canonical_line: &str) -> String; // first 16 hex of SHA-256
pub fn position(arr: &Arrangement, text: &str) -> Result<f64, String>; // "9.3.1" → beats
```
- `apply` takes `&Revision`, not `&DataSet`: lane kinds and units come from the IR's lane declarations.
- The rules table of design §7.1 holds as written (ids, kinds and units, create, both resize edges, split, join,
  duplicate rounding to bars, paste, quantize).
- `ClipsEdited { edited: Edited, made: Vec<(LaneAddr, String)> }`: the new ids, so the UI selects them.
- Reuse `notes::tidy` and `describe` (make them `pub(crate)`, and extend `describe` to control and audio clips).

**Automation** (`automate.rs`):
- `name_of` (D6): the key from the control's slot (`decay`), with the device's name in front only on a clash
  (`pluck-decay`); `{ name: "Decay" }` only when the key isn't the label in sentence case.
- `pub fn remove(s: &mut Session, module: &str, lane: &str, beat: Option<f64>) -> Result<Edited, EditError>`: one
  `Confined` Tx that replaces the `lane.control(key, …)` read with a literal of the lane's value at `beat` (the
  fallback when `None`), in the unit's spelling, and removes the lane file. The track's `lane` parameter stays.
- `view.rs`: an input fed by a lane says which, `InputPort.lane: Option<{module, lane}>`, for "Follows the Cutoff
  lane" and Show lane.

**Server**:
- New `crates/server/src/routes_8b.rs` with its own router and its own WRITES list (§7), merged in one line.
- New `crates/server/src/graph/clips.rs`: `impl Graph { pub fn clips(&self, token, gesture, seen) -> Result<Value>;
  pub fn remove_automation(&self, token, revision, module, lane, beat) -> Result<Value> }`, both through
  `Actor::edit_and`.
- `actor.rs` `timeline()`: when a lane's JSON is rebuilt, each clip object gets `sha` (computed only then, so an
  unchanged lane costs nothing).

**CLI**: `m2 clips <project> <gesture> …` (`split bass/notes:k3f9q2zt --at 9.3.1`; `duplicate|join|move|delete|
quantize …`; `--beats` for plain beats), `m2 automate --remove <project> <track>/<lane> [--at <bar.beat>]`. Both take
the lock.

**Markers (W2, after taste-next's `layout.rs`)**: `layout::markers` (add, move, rename, delete) as one more layout
`Change`, through taste's `POST /api/graph/layout` with `markers`, and `m2 marker add|move|rename|delete`.

### 3.3 What the UI will need later

| Kind | What |
|---|---|
| route | `POST /api/graph/clips` `{session, gesture, seen: {clip id: sha}}` → `{file, before, after, revision, made: [{module, lane, id}]}`. In WRITES. A changed clip answers 409 with `EditError::Changed`'s words ("The Bass clip at bar 9 was changed…") |
| route | `POST /api/graph/automate/remove` `{session, revision, module, lane, beat}`. In WRITES |
| timeline | each clip carries `sha`; `end` exists already (the End flag) |
| view | inputs driven by a lane carry `lane: {module, lane}` |
| automate | the answer's lane key and name follow D6 (`decay`, "Decay") |
| markers | `layout.markers` in taste-next's layout view (W2) |
| unchanged | `/api/graph/lane` stays until the roll sends `Notes` gestures (Q9) |

### 3.4 Tests

| Where | Tests |
|---|---|
| `crates/editor/src/clips_tests.rs` | ids: `a_move_keeps_its_id`, `a_move_onto_a_taken_id_mints_one`, `every_copy_mints`; trims: `a_left_trim_keeps_notes_in_place`, `trimming_back_brings_the_notes_back`; splits: `a_split_shortens_the_crossing_notes`, `a_split_control_clip_stays_continuous`, `a_split_audio_clip_plays_the_same_samples` (`examples/resample`); placing: `a_duplicate_lands_after_the_selection`, `a_duplicated_section_lands_on_the_next_bar`, `a_marquee_duplicate_keeps_its_empty_bars`, `a_paste_mints_and_keeps_offsets`; joins: `join_makes_one_clip_and_keeps_every_note_in_time`, `control_and_audio_clips_dont_join`; `quantize_to_sixteenths_and_triplets`; `kinds_and_units_never_mix`; `touching_a_lane_gives_its_anonymous_clips_ids`; conflicts: `a_gesture_on_a_changed_clip_is_changed`, `an_outside_edit_to_another_clip_merges`, `undoing_after_an_outside_change_to_the_clip_conflicts`; `apply_is_pure` (same input, same edits) |
| `crates/editor/src/automate_tests.rs` | `the_lane_is_named_after_the_control`, `remove_automation_puts_the_heard_value_back` (and removes the file; undo restores both), `remove_automation_at_a_clip_uses_its_value` |
| `crates/editor/src/view_tests.rs` | `an_input_a_lane_drives_names_the_lane` |
| `crates/server/src/graph/tests.rs` | `a_clip_gesture_takes_the_data_path` (no QuickJS run, one `SwapLane`), `the_timeline_carries_each_clips_sha`, `a_stale_sha_is_refused_with_words` |
| `apps/m2/tests/clips.rs` (new) | each gesture by CLI; positions as bar.beat.sixteenth; the lock refusal |

### 3.5 Contract edits

WRITE-10 (rows: every clip gesture, join included, `Confined { the lanes it touches }`; remove automation `Confined {
the lane expression, the lane removed }`; markers, layout only, with W2), WRITE-12 *Today* (clip gestures carry each
touched clip's hash; the roll's whole-lane write still names its revision until the UI moves), FMT-12 ("a builder
gives ids to the anonymous clips of each lane it writes"), HOME-2 *Today* (Remove automation is built; latches
aren't), FMT-19 (markers, with W2), WRITE-18 (`clips`, `automate --remove`, `marker`), `docs/gaps.md` #9, #14, #17.

---

## 4. 8b.6 Audition

### 4.1 What exists

| Piece | State | Where |
|---|---|---|
| Engine-wide note ids | built: `Renderer.next_note` | `core/runtime/src/lib.rs` (`Renderer`) |
| Schedulers that persist across plans, and grow | built: `LaneMove`, `NoteScheduler::take_state` | `plan.rs`, `sched.rs:252` |
| Fixed event buffers that refuse an On when full | built: `Events`, `dropped_ons` | `core/runtime/src/event.rs:66` |
| The engine runs while stopped, and idles | built: `Engine::is_idle`, `Renderer::can_idle` | `engine.rs:266`, `lib.rs:2235` |
| Panic | built: `Player::panic`, `Renderer::panic` (resets schedulers) | `core/play/src/lib.rs:921`, `lib.rs:2212` |
| Generation checks on lane messages | built for `SwapLane` (refused when the plan differs) | `engine.rs:450` |
| A window's id, and its close | built: `?client=`, `Graph::client_gone` | `routes.rs:147`, `graph.rs:1090` |
| A 300 ms server thread for expiring gestures | built: `music2-tell` → `Graph::expire` | `crates/server/src/lib.rs`, `graph.rs:1041` |
| `Msg::Live`, a live table, `Player::live` | missing | — |
| Reading WebSocket messages | missing: incoming text is ignored | `routes.rs:183-188` |
| `live` in the status | missing | `routes.rs:239` |

### 4.2 What the back end needs

**Runtime**:
- `engine.rs`: `Msg::Live { generation: u64, slot: u32, ev: LiveEv }`. Structural: it queues in order behind
  activations, so the plan it names is the one playing when it applies. Another generation's is refused and counted.
- `pub enum LiveEv { On { source: u32, handle: u32, pitch: f32, velocity: f32 }, Off { source: u32, handle: u32 },
  Release { source: u32 } }` (handles are the window's, so On and Off carry the source too).
- `sched.rs` `NoteScheduler`: `live: [Option<LiveNote>; 16]` (`{source, handle, id, pitch}`) and a fixed `pending`
  of 32; events capacity `caps.events() + 32`. `pub(crate) fn live(&mut self, ev: LiveEv)`. `run()` writes pending
  Offs, then Ons, at the first run's frame 0, before the transport's notes, with ids from `next`. Jumps never touch
  `live`. `take_state` copies it; `reset` (panic) clears it with Offs. A full table refuses the On and counts it.
- `Renderer::live(&mut self, slot, ev)`, `Renderer::live_notes() -> usize`; `can_idle` is false while any live note
  sounds; an activation that releases a lane's consumer (`Entry.release`) clears that lane's live table.
- **core/play**: `Player::live(generation, slot, ev) -> Result<u64>`; `Shared.live: AtomicU32`, published each
  callback; the "nothing plays" match (`core/play/src/lib.rs:444`) retires `Live` unapplied.

**Server**, new `crates/server/src/graph/audition.rs`:
- `impl Graph { pub fn audition(&self, client: &str, msg: Audition) -> Result<Option<Value>> }`, with
  `Audition { module, lane, on: Vec<{h, pitch, velocity}>, off: Vec<u32>, keep: Vec<u32> }`.
- It numbers each client as a source, resolves `(module, lane)` to a slot through `PlaysView`'s lane refs (C14), and
  sends `Live` under the control lock. It never takes the session (RENDER-29). It works read-only.
- A lane nothing reads answers once `{"t":"audition","silent":"Nothing plays this lane"}`.
- The watchdog: `expire()` sends `Release` for a source with held notes and no keepalive for 10 s; `client_gone`
  releases at once.
- `routes.rs` `ws_loop`: parse incoming text `{"t":"audition",…}`, call `audition` on a blocking task, send any reply
  to this socket only. Also `POST /api/graph/audition` (`{client, …}`, not in WRITES) for tests and agents.
- `GET /api/graph/status` gains `live`.

### 4.3 What the UI will need later

| Kind | What |
|---|---|
| WebSocket out | `{"t":"audition","module":"bass","lane":"notes","on":[{"h":7,"pitch":45,"velocity":0.8}],"off":[6]}`; keepalive `{"t":"audition","keep":[7]}` every 2 s while held |
| WebSocket in | `{"t":"audition","silent":"Nothing plays this lane"}`, to that window only |
| route | `POST /api/graph/audition` (same body plus `client`) |
| status | `live`: live notes sounding |
| hints | "Muted · audition is silent" and the others come from `state.overlays.heard` (§2.3) |
| panic | taste-next's `POST /api/graph/panic`; the runtime ends live notes on panic |
| toggle | `view.json`'s `audition` (taste-next) |

### 4.4 Tests

| Where | Tests |
|---|---|
| `core/runtime/src/tests/audition.rs` (new) | `a_live_note_sounds_and_ends`, `live_ids_never_meet_scheduled_ids` (the probe plugin sees no repeated id), `a_live_note_survives_stop_seek_and_wrap`, `a_live_note_moves_with_its_lane_across_an_activation`, `a_replaced_consumer_ends_its_live_notes`, `a_full_table_refuses_and_counts`, `release_ends_one_sources_notes`, `panic_ends_live_notes`, `a_live_note_wakes_an_idle_engine`, `another_generations_live_is_refused` |
| `core/play/tests/no_alloc.rs` | `Live` messages while playing and stopped |
| `core/session` | renders gate (offline never gets `Live`) |
| `crates/server/src/graph/tests.rs` | `audition_never_waits_for_a_compile` (an On heard during a 1.5 s compile), `a_closed_window_releases_its_notes`, `the_watchdog_releases_a_quiet_window`, `a_lane_nothing_reads_says_so`, `audition_works_read_only` |

### 4.5 Contract edits

MON-7 (a live note: its source, slot, room of 16, ids, what ends it), RENDER-9 ("every note a lane scheduled"; live
notes keep sounding through a jump), RENDER-14 (live notes merge at the block line, frame 0, Offs first), RENDER-26a
(a live note ends only by its release, its source going, panic, or an activation that replaces its consumer),
RENDER-28 and the Engine shape (`Live(generation, slot, LiveEv)`), RENDER-29 *Today* (audition never takes the
session), the Planes table's control row (audition), `docs/gaps.md` #8.

---

## 5. 8b.8 Meters and the DSP load

### 5.1 What exists

| Piece | State | Where |
|---|---|---|
| Reading a node's last block off the audio thread | built for the gates: `Renderer::output_of` | `core/runtime/src/plan.rs` |
| xruns and device errors in the status | built: `Activity` | `crates/server/src/graph.rs:159`, `routes.rs:239` |
| A player that plays in real time with no device | built: `M2_AUDIO=none` | `core/play/src/lib.rs:774` |
| Broadcast events to every window | built: `AppState.events` | `crates/server/src/lib.rs` |
| Whether the engine idles | built inside the engine (`Engine::is_idle`), not published | `engine.rs:266` |
| Taps, the meters message, a load figure | missing | — |

### 5.2 What the back end needs

**Runtime**:
- New `core/runtime/src/taps.rs`: `pub struct TapSlots { peaks: Box<[AtomicU32]>, keys: Vec<(String, u32, u8)> }`
  (one slot per channel), `pub enum Tap { Out, Track(String), Node(String /*FNode name*/) }`.
- `Renderer::set_taps(&mut self, taps: &[Tap])` before the renderer is sent (allocates off the audio thread);
  `Renderer::taps() -> Arc<TapSlots>`. `Track(k)` taps k's overlay nodes (the largest of its sites); `Out` taps the
  song's output; `Node` is for the channel gain after 8b.3 (Q2).
- In `render_block`, after `graph.process`: for each tap, each channel's largest `|x|` over the block, `fetch_max` of
  its bits (non-negative `f32`s order as `u32`s). No lock, no allocation. Offline renders have none.

**core/play** (the DSP load, C2):
- In `fill_in`: the callback's time (`now_ns()` at the end minus `ns` at the start), `fetch_max` into
  `Shared.longest_ns`; a callback longer than its period adds to `Shared.late`.
- `Player::load() -> Load { longest: f32 /* share of the period */, late: u64 }` (it swaps `longest_ns` to 0).
- `Shared.idle: AtomicBool` from `Engine::is_idle()`; `Player::idle()`.

**Server**, new `crates/server/src/graph/meters.rs`:
- `impl Graph { pub fn meters(&self) -> Option<Value> }`: `None` while nothing is open or the engine idles;
  otherwise it swaps every tap of the plan that plays to 0 and reads the load, keeping the last 30 loads so `load` is
  the longest of the last second.
- `actor.rs`: each new renderer gets `set_taps([Track(k) for each track with a site] + [Out])` before it is sent;
  `PlaysView` carries its `TapSlots`.
- `crates/server/src/lib.rs`: a third thread, `music2-meters`, every 33 ms: `events.send(meters)`. Never `announce`,
  so no meters are kept for a window that connects later.
- Status: `load`.

### 5.3 What the UI will need later

| Kind | What |
|---|---|
| WebSocket in | `{"t":"meters","peaks":{"kick":[l,r],…,"out":[l,r]},"dsp":{"load":0.23,"dropouts":0}}`, about 30 a second while the engine runs, none while it idles |
| status | `load`, `xruns` (built), `device_errors` (built) |
| UI-side only | peak hold, the clip dot, the DSP pill's colours, the device popover |

### 5.4 Tests

| Where | Tests |
|---|---|
| `core/runtime/src/tests/taps.rs` (new) | `taps_change_no_bit`, `a_tap_reports_its_blocks_peak`, `a_muted_tracks_tap_reads_silence` |
| `core/play/tests/no_alloc.rs` | taps and the load timer in the callback |
| `core/play/src/tests.rs` | `the_load_is_the_longest_callback`, `the_engine_says_when_it_idles` |
| `core/session` | renders gate |
| `crates/server/src/graph/tests.rs` | `meters_are_sent_while_playing_and_stop_when_idle`, `a_new_plan_moves_the_taps`, `muting_kick_floors_its_meter_and_the_bass_still_moves` |

### 5.5 Contract edits

The Deferred table: "Render taps, meters, track freezing, bounce as a Tx" splits; taps and meters are built, freezing
and bounce stay deferred ("the resample loop in the UI"). The Engine shape names `taps`. `docs/gaps.md` #20 (meters
built; loudness for export still missing). Optionally a line under RENDER-28 that the callback's time is measured with
the clock only.

---

## 6. The slices, in order

Each lands green alone: its crates' tests pass, `core/session/tests/renders.json` is unchanged, and
`e2e/renders.sha256` isn't touched. A slice that touches `core/*` edits the contract in the same commit. Each slice
adds its section to `docs/designs/step8/build-notes-8b-backend.md` in the same commit.

| # | Feature | What lands | Main files | Test these crates | Contract | Size |
|---|---|---|---|---|---|---|
| **B1** | 8b.4 | `Module.muted`, the SDK option, `Flat.overlays`, `Flat.side_inputs`, `Flat.muted`; the delta's `Muted` (still structural here) | `core/ir/src/{lib,check}.rs`, `core/script/sdk/core.ts`, `core/script/src/lib.rs`, `core/session/src/tx/delta.rs` | `m2-ir`, `m2-script`, `m2-session` | PKG-13, ID-11, MON-5 (option) | M |
| **B2** | 8b.4 | The overlay op in every plan, offline included; overlay cells; `set_overlays`; fades and ghosts read through it; the click test | `core/runtime/src/{overlay,lib,cells,plan}.rs`, `core/play/tests/mute.rs` | `m2-runtime`, `m2-play`, `m2-session` | MON-1, RENDER-22a, RENDER-23, MON-5 *Today* | L |
| **B3** | 8b.4 | Server overlays: state, solo rule, `.music2/overlays.json`, `POST /api/graph/overlay`, new plans' values, `Muted` as cell writes (now not structural), rekey, `overlays.active` on export | `crates/server/src/graph/{overlay,actor,lease}.rs`, `routes_8b.rs`, `core/session/src/tx/delta.rs` | `m2-server`, `m2-session` | MON-3, MON-4, WRITE-8, Planes | L |
| **B4** | 8b.4 | `mute_in_song` builder, `feeds`, `POST /api/graph/track/mute`, `m2 mute-in-song` | `crates/editor/src/mute.rs`, `graph/overlay.rs`, `routes_8b.rs`, `apps/m2` | `m2-editor`, `m2-server`, `m2` | WRITE-10, WRITE-18, MON-2, gaps #4 | M |
| **B5** | 8b.4 | The sidechain gate on `env` and `follow`; live equals offline; a track-wrapped `tone(hz)` in the gates' songs | `core/session/tests/fixtures/sidechain/{env,follow}/`, `core/session/tests/mute_sidechain.rs`, `core/play/tests/mute.rs`, `crates/gates/src/songs.rs` | `m2-session`, `m2-play`, `m2-gates` (build) | the gate's row: two of three clauses | S |
| **B6** | 8b.5 | `clips.rs`: create, move (copy, across lanes), resize, delete, duplicate, paste, set; `apply` and `edit`; `POST /api/graph/clips`; `m2 clips` | `crates/editor/src/clips.rs`, `notes.rs` (`pub(crate)`), `graph/clips.rs`, `routes_8b.rs`, `apps/m2` | `m2-editor`, `m2-server`, `m2` | WRITE-10, FMT-12, WRITE-18 | L |
| **B7** | 8b.5 | Split, join, quantize, `Notes`, `Points`; each clip's `sha` in the timeline; `seen` | `clips.rs`, `actor.rs` (`timeline()`) | `m2-editor`, `m2-server` | WRITE-12 *Today*, gaps #9, #17 | M |
| **B8** | 8b.5 | Automate's lane names (D6), `automate::remove`, its route and CLI, lane-driven inputs in the view | `automate.rs`, `view.rs`, `graph/clips.rs`, `apps/m2` | `m2-editor`, `m2-server`, `m2` | HOME-2 *Today*, WRITE-10, gaps #14 | M |
| **B9** | 8b.6 | `Msg::Live`, the live table, `Player::live`, `live` published, panic and idle | `core/runtime/src/{engine,sched,lib}.rs`, `core/play/src/lib.rs` | `m2-runtime`, `m2-play`, `m2-session` | MON-7, RENDER-9, 14, 26a, 28, Engine shape | M |
| **B10** | 8b.6 | Server audition: WebSocket reading, the route, sources, watchdog, close, `silent`, status `live` | `graph/audition.rs`, `routes.rs` (`ws_loop`), `routes_8b.rs`, `lease.rs` (lanes in `PlaysView`) | `m2-server` | RENDER-29, Planes, gaps #8 | M |
| **B11** | 8b.8 | Taps; the callback timer; `idle` published | `core/runtime/src/{taps,lib}.rs`, `core/play/src/lib.rs` | `m2-runtime`, `m2-play`, `m2-session` | Engine shape | M |
| **B12** | 8b.8 | Server meters: tap choice, the 30 Hz thread, the `meters` event, status `load` | `graph/meters.rs`, `crates/server/src/lib.rs`, `actor.rs` | `m2-server` | Deferred table, gaps #20 | S |
| **W1** | 8b.4 | std's keyed compressor (`compress` gains `key?: Audio`, sidechain; new kernel `std:compress_keyed`); the `keyed` fixture; the gate's third clause | `std/src/fx.ts`, `std/kernels-src/src/fx.rs`, `std/m2pkg.toml`, `std/kernels/std.wasm` | `std/build.sh`, `std/build.sh --check`, `m2-session`, `m2-runtime` | PKG-13's example; gate 8 row whole | S |
| **W2** | 8b.5 | Markers in `layout.json` and `m2 marker` | taste's `crates/editor/src/layout.rs` | `m2-editor`, `m2-server`, `m2` | FMT-19, WRITE-10, WRITE-18 | S |
| **End** | all | The full check (`ci/check.sh`), the gates owed (`m2-gates --server`), the build notes' last section | — | everything | — | — |

**When W1 and W2 run:** W1 after sounds-merge's std 0.2 is on `main`; merge `main` into `8b-backend` first (a merge
commit, never a rebase). W2 after taste-next's 8b.1 is on `main` (or is merged in). If the track ends before that,
hand W2 to taste-next: it is one small change to its own `layout.rs`.

**Why this order:** it follows the audit's ranks (mute and solo first, clips next, audition, then meters). B1 to B3
take the one large risk (the op under step 7's fades) first. Meters come last because their taps sit on B2's overlay
nodes.

---

## 7. taste-next: what we depend on, and how to avoid conflicts

### 7.1 What the back ends depend on

| From taste-next | Used by | Until it lands |
|---|---|---|
| 8b.1 `layout.rs` (markers kept raw), `plan::base_from_disk` | W2 markers | markers wait |
| 8b.1 track delete and add | §6.5: a builder delete drops the track's overlay entries; an add drops stale ones | we expose `Graph::forget_overlays(keys)`; the merge calls it from taste's `track/delete` and `track/add` (two lines) |
| 8b.1 `mix.rs` (channel per track) | `feeds` for a fed node in `song` (name the channel's track, not `song`); shared strips | `feeds` names `song` (Q6) |
| 8b.1 `rename.rs` changes | `mute.rs` imports `rename::track_call` | if 8b.1 moves it, one import line at the merge |
| 8b.2 `graph/view_file.rs` | `audition` toggle, `export.ask`, heights | UI only; nothing here reads it |
| 8b.2 `POST /api/graph/panic` | ending live notes | the runtime does it; no route here |
| 8b.3 std mixer `db` not optional (a gain on every channel) | post-fader track meters | tracks are tapped at their overlay (post-mute, pre-fader) (Q2) |
| 8b.3 `MixView.strips[].muted_in_song` | the struck name | the UI reads `state.overlays.muted_in_song` instead |

None of B1–B12 needs a taste-next builder to build or to pass its tests.

### 7.2 Shared files and the rule for each

| File | taste-next touches it for | Our rule |
|---|---|---|
| `crates/server/src/routes.rs` | new routes and the fixed-size `WRITES: [&str; N]` | all our routes in a new `routes_8b.rs` with `pub fn router()` and its own `WRITES_8B: &[&str]`. In `routes.rs`: one `.merge(routes_8b::router())` line, and `\|\| routes_8b::WRITES_8B.contains(&path)` at the check (`:132`). Only B10 touches `ws_loop` |
| `crates/server/src/graph.rs` | song bar methods, state fields | new `impl Graph` blocks live in `graph/{overlay,clips,audition,meters}.rs` (a child module may impl its parent's type). In `graph.rs`: `mod` lines, one `ControlPlane` field, one line in `Inner::state` (`state["overlays"]`) |
| `crates/server/src/graph/actor.rs` | `shown()` state, `timeline()`'s track pattern | touch `settle`, `reach_to`, `value_writes` and the rebuilt-lane branch of `timeline()` only; never `shown()` or `TimelineTrack`'s pattern |
| `crates/editor/src/lib.rs` | `pub mod tracks; layout; mix; song;`, the `Gestures` trait | add our `pub mod clips; pub mod mute;` at the end of the list. **Never add to `Gestures`**; call module functions |
| `crates/editor/src/timeline.rs` | `TimelineTrack.colour`, order | don't touch it: the clip `sha` goes in at the server (B7) |
| `crates/editor/src/notes.rs`, `automate.rs`, `view.rs` | not touched by 8b.1–8b.3 | ours |
| `apps/m2/src/main.rs` | arms after `label`, usage lines after `label` | our commands in a new `apps/m2/src/edits.rs`, one dispatch arm before the match's last arm, usage lines after `plugin-state` |
| `apps/m2/tests/lock.rs` | adds its commands | never edit it; each new test file checks its own lock refusal |
| `core/script/sdk/core.ts` | PKG-7 (editor categories on instruments) | only `TrackOptions` and `track()`'s options line |
| `core/session/src/tx/delta.rs` | `scope(…, within, …)` | only `structural()` and `modules()` |
| `docs/architecture.md` | WRITE-10's table, WRITE-18's list, FMT-19, HOME, PKG-3/5/7, ID-18, DIAG | edit MON, RENDER, PKG-13, ID-11, WRITE-8/12, FMT-12, HOME-2 *Today* freely. For WRITE-10 and WRITE-18 add a separate bullet at the end of the rule ("From 8b.4/8b.5: …"), not rows inside the shared table or list |
| `docs/gaps.md` | #1, #6, #7, #10 | only #4, #8, #9, #14, #17, #20 |
| `std/` | 8b.3's mixer `db`; sounds-merge's std 0.2 | nothing until W1 |

---

## 8. Open questions, each with a recommended default

| # | Question | Recommended default |
|---|---|---|
| Q1 | Build 8b.4, 8b.6 and 8b.8 before 8a, though the order of work puts 8a first? | **Yes.** The runtime is single-threaded today, so nothing needs labels. 8a labels overlay nodes and writes taps inside its tasks when it lands |
| Q2 | Where are track meters tapped before 8b.3's channel gains exist? | **At the track's overlay nodes** (after mute, before the fader). After the 8b.3 merge, the server switches to `Tap::Node(<channel gain>)`: a server change only |
| Q3 | DSP load with no 8a load record? | **A callback timer in `core/play`**: the longest callback of the last second as a share of the period; dropouts = xruns plus late callbacks. 8a later replaces its source |
| Q4 | Keep `M2_AUDIO=paced`? | **No.** `none` already renders in real time |
| Q5 | When does the keyed compressor land? | **W1, after std 0.2 is on main**, merged in. `env` and `follow` prove two clauses first. Ask sounds-merge not to bump `std.wasm` twice: one rebuild after the merge |
| Q6 | `feeds` for a fed node in `song` before mix.rs is here? | **Name `song` and the component** ("Duck"); map to the channel's track after the merge |
| Q7 | Overlay entries when a track is deleted or added with 8b.1's builders? | **`Graph::forget_overlays(keys)` now**, kept in memory for the delete's undo; wired into taste's routes at the merge |
| Q8 | How does a clip gesture say which version of a clip it saw? | **A `sha` per clip** (16 hex of SHA-256 of its canonical line), added at the server only to rebuilt lanes' JSON. The CLI and agents plan on the accepted revision and send no `seen` |
| Q9 | Delete `/api/graph/lane` and `write_notes` now, as design §7.1 says? | **No.** The web's roll uses it (`web/src/lib/graph.ts:501`). Delete it in the UI slice that moves the roll to `Notes` gestures; WRITE-12's exception goes then |
| Q10 | How is Remove automation addressed, and at what value? | **By lane** (`module`, `lane`), the value at `beat` (the playhead) from the route; the CLI's `--at`, else the lane's fallback |
| Q11 | Markers: here or in taste-next? | **Here as W2 once `layout.rs` is on main**; else taste-next adds them to its own `layout.rs` |
| Q12 | Audition over the WebSocket only, as the design says? | **Both**: the WebSocket for the app, and `POST /api/graph/audition` for tests and agents, through one handler |
| Q13 | A new event type for overlay changes? | **No.** The usual `{"t":"graph"}` event, then the window reads `state.overlays` |
| Q14 | Mute in song for offline renders: a `prepare` argument (`OverlayInit`) or the graph? | **The graph**: `Flat.muted`, which every offline path already passes. Live plans use `Renderer::set_overlays` |
| Q15 | A ghost or a fade that reads a muted track's audio: let it through for its 10 ms window? | **No.** Ghost and fade inputs read through the overlay; B2's `an_edit_while_muted_keeps_the_track_silent` holds it |
| Q16 | `@m2/core`'s version for the `muted` option? | **1.1.0 → 1.2.0** (an addition, FMT-2). If sounds-merge bumps it for `time.playing`, take the higher at the merge |
| Q17 | Live notes on Stop, Seek and loop wraps? | **They keep sounding** (design §8.1): a held key is a hand, not the song |
| Q18 | The gates' audition latency gate (G13, `m2-gates --server audition`)? | **Record it at the End step if time allows**; not a slice. Linux numbers are information only (PLAT-1) |

---

## 9. How to run things

- Environment for every cargo or music2 command:
  `CARGO_HOME=/home/jerry/.cargo RUSTUP_HOME=/home/jerry/.rustup`. Anything that runs music2 (server and CLI tests):
  `HOME=/home/jerry/m2-runs/be-scratch/home M2_AUDIO=none`. Builds `--locked`; never `cargo update`.
- One target folder: the worktree's own `target/`. Delete it when the track ends (scratch builds once filled 390 GB).
- A slice's tests, for example B2:
  `cd /home/jerry/m2-runs/wt/be && CARGO_HOME=… RUSTUP_HOME=… HOME=/home/jerry/m2-runs/be-scratch/home M2_AUDIO=none cargo test --locked -p m2-runtime -p m2-play -p m2-session`
- Long runs (the server's tests, the End check) through the tmux helper:
  `TW_WAIT=2400 ~/m2-runs/tw be-b3 'cd ~/m2-runs/wt/be && … cargo test --locked -p m2-server'` (logs in
  `~/m2-runs/logs/`).
- W1 only: `std/build.sh`, then `std/build.sh --check`, in the same commit as the change.
- The End step: `ci/check.sh` (tests, clippy, std, renders, browser suites as they are).
- Commits: small and green; a plain subject, a blank line, a short body, then the two trailer lines. Never push, never
  rewrite a commit; take other tracks' work by merging `main`.

---

## Attack

Checked against the code at `09b2dfd` (the worktree), taste-next at `cf3115b` and sounds-merge at `52801b6`. Read
only; nothing was built or run.

### In short

- **The plan is mostly right.** Its claims C1 to C16 hold, but for one (G8: taste-next does touch `view.rs`).
- **Ten things are wrong.** Seven would break sound or the build: the delta's `structural()` (A1), overlay cells as a
  `CellAt` variant (A2), the fade's old side (A3), where an overlay sits in the order (A4), `feedback` consumers (A5),
  the first renderer at open (A6), and `set_overlays` writing slots (A7). Three are tests or rules that can't hold (A8 to A10).
- **Some things are missing:** meters can push `graph` events out of a slow window (M1); a few file and thread rules
  (M2 to M6).
- **Some slices aren't green alone,** or will clash with taste-next at the merge. Each has a one-line fix (G1 to G10).
- **Time is the biggest problem.** At the plan's own sizes the 12 slices are 6 to 7 agent-days, done one after
  another. The owner wants the app in 2 days. Simplifications (S3 to S6) save about a day. The owner must pick what
  goes first (S1).

### A. Wrong as written

| # | What | Where in the code | Fix |
|---|---|---|---|
| A1 | B3 makes `structural()` ignore `Muted`. But `is_empty()` is built on it, and `Expect::Identical` and `Expect::Values` judge with `is_empty()`. A Tx that mutes a track would then pass as "changes nothing". | `core/session/src/tx/delta.rs:86-91`, `tx/expect.rs:138`, `:147` | Leave `structural()` as it is. In `reach_to`, a delta is reachable by cell writes when it adds, removes and rewires nothing and every module change is `Muted`. `delta.rs` changes only in `modules()` and the `Graph` it reads. |
| A2 | Overlay cells can't be a new `CellAt` variant. The server matches `CellAt` with no wildcard. B2 would break `m2-server`'s build, and B2 doesn't build it. | `crates/server/src/graph/actor.rs:216-221` | Keep `CellAt` and `cells::cells(flat)` as they are. `Cells` keeps a second table: an id past the value cells is an overlay exec node. `same_layout` and `value_writes` never see overlays. |
| A3 | A fade's **old** side leaks a muted track. The plan puts the overlay under the fade's `new` side only. When the old edge crossed tracks, `old` reads the track directly, unmuted, for 5 to 10 ms. Example: Kick is muted, and an edit puts an effect on the bus input that read Kick. | `core/runtime/src/keep.rs:365` (`old: zero(&remap_input(was))`) | One rule in B2: **every audio edge that crosses tracks reads through an overlay.** (1) Each site of the new `Flat` gets a celled overlay. If a fade sits on that input (a changed edge or a fade-in), the overlay goes on the fade's `new` side; the same for a fade on `Out`. (2) A fade's `old` side, and a ghost's input, that was a site in `k.old.overlays` get a window-only overlay with no cell. Its value is the new plan's value for the same (track, consumer module), else the mute-in-song value. `k.old` is the old `Flat`, so its site list says all this. Set a fade's sides directly: `Graph::rewire` has no arm for `Exec::Fade`. |
| A4 | "Just before the consumer" in `order` is wrong for a `feedback` consumer. A feedback node runs **before** its source, and its ring is written after the whole order. An overlay just before it reads last block's source: one block late, and the bits change. | `core/runtime/src/lib.rs:1253-1265` | Put each overlay **right after its source** in `order` and `order_after`. That is right for every consumer. Window-only overlays (A3) go in `order` only. |
| A5 | `Consumer` has no `feedback` case. `FOp::Feedback { input }` can read another track's audio, and the runtime already rewires `Edge::Feedback`. | `core/ir/src/check.rs:602`, `core/runtime/src/lib.rs:1341` | Add `Consumer::Feedback { node }`. The four cases then match the runtime's `Edge`: `Input`, `Capture`, `Feedback`, `Out`. |
| A6 | The first renderer at open misses the overlays. `Actor::play` builds and loads it outside `settle`, so "beside the lease loop" never runs there. | `crates/server/src/graph/actor.rs:1021-1047` | Read `.music2/overlays.json` before that build, and call `set_overlays` before `o.load`. |
| A7 | `set_overlays` must not write `CellSlots`. A slot write publishes a seq and marks the cell dirty, so the `heard` stamps would be wrong. | `core/runtime/src/cells.rs:163-175` | Set the table's value and the node's `cur` and `target` only, as `Cells::set_now` does for leases. |
| A8 | `a_split_audio_clip_plays_the_same_samples` can't hold in general. Each clip renders on its own, with its fades and its resampling. The design's offset rule ignores `pitch` and `reverse`. | `core/runtime/src/audio.rs:118`, `:302-315`, `core/song/src/lane.rs:171` | Split and left-trim refuse an audio clip with `reverse` or `pitch ≠ 0`, in words. Test exact samples only for a split on a sample line, at the asset's own rate. |
| A9 | The ducking gate's "A′ and B are bit-identical" depends on signed zeros. The overlay gives `x × 0`, with the sign of `x`; std's `gain` writes `+0.0` to a right channel it doesn't have. `bass + (−0)` and `bass + (+0)` differ when the bass is `−0`. | std `kernels-src/src/lib.rs:385-393` | Compare samples with `==` on `f32` (`-0` equals `+0`), not bits. The same for "live equals offline". |
| A10 | "`reset` (panic) clears the live table with Offs" goes against `NoteScheduler::reset`, which ends notes without Offs (the voices are reset, and a plugin's reset ends its keys). | `core/runtime/src/sched.rs:275-280` | Clear the live table without Offs, as `sounding` is cleared. |

### M. Missing

| # | What | Fix |
|---|---|---|
| M1 | Meters on `AppState.events`, a broadcast of 256. `ws_loop` skips a `Lagged` error (`routes.rs:177`). A window that stalls about 8 s at 30 meters a second loses `graph` events, and shows old data. | Meters get their own `tokio::sync::watch` channel: only the latest value is kept. `ws_loop` selects on it too. The meters thread does nothing while `receiver_count()` is 0. |
| M2 | Where `.music2/overlays.json` is written. Disk I/O under the control lock would hold up knob drags and `settle`. | `overlay()` only marks it changed. The `music2-tell` thread (every 300 ms) writes it: a temp file in `.music2/`, then rename. The design asked for "at most every 200 ms"; 300 ms is fine. |
| M3 | `Player::load()` swaps the longest time to 0. If the status route and the meters thread both call it, each steals the other's peak. | Only the meters thread calls it. It keeps the last second. `status.load` reads the meters thread's figure. |
| M4 | `Live` while the engine is held for an export. Nothing renders while held, so a note would sound after the export ends, maybe seconds later. A `Live` waiting at the head would also block the lane swaps that apply while held. | The engine refuses `Live` while held, and counts it. Otherwise `Live` is always ready: it retires nothing, so it needn't wait for stash room (`engine.rs:418`). |
| M5 | `muted` on a module that isn't a track: no site would ever use it. | The checker refuses it (`code.check`), as it refuses `muted` on `song`. |
| M6 | Where the server keeps its state. `ControlPlane` outlives a project. A mute toggle reached by cell writes needs the new revision's mute-in-song list. | `Overlays` lives in `Published` (made new at each open). The server keeps the accepted revision's `muted` list beside it. The `Reach` branch of `settle` writes the overlay cells whose value changed, under the same lock. |
| M7 | D6 doesn't say how a dictionary control's lane is named. `e2e/s6-builders.mjs:133` and `:159` expect today's names (`hats-pan`, `wobble-hi`). | A dictionary slot is `<entry>-<control>` (`hats-pan`, as today). Otherwise the slot (`hi`), with the call's binding in front only on a clash. B8 updates the expected text at `:159` and runs that browser suite at the End. |
| M8 | The DIAG code `overlays.active` needs its row in the contract's codes table. | Add it with MON-4 in B3. |

### G. Not green alone, and merges

| # | What | Fix |
|---|---|---|
| G1 | A slice tests only its own crates. A change can still break a crate it didn't test (A2 is one). Clippy and the offline renders run only at the End. | Each slice also runs `cargo check --locked --workspace --all-targets` and `cargo clippy --locked -p <crates> --all-targets -- -D warnings`. B2, which changes every offline plan, also runs `e2e/baseline.sh` and checks `e2e/renders.sha256` (about 1 minute). |
| G2 | B1 makes `muted: true` legal code that does nothing until B2. | B1 is the IR, the checker and the delta only. The SDK option and `SDK_VERSION` 1.2.0 move to B2, with the op. Each commit then means what it says. |
| G3 | B2's click test needs the track-wrapped tone that B5 adds to `crates/gates`. But `core/play`'s tests can't use `crates/gates`. `core/session/tests/mute_sidechain.rs` would be a new test target with no helpers: `gate_tests`' helpers are `pub(crate)`. | Write the tone song in `core/play/tests/mute.rs` itself. Put the ducking gate in the same file: `tests/common` already compiles a project (`fixture`) and drives the player. Keep the fixtures in `core/session/tests/fixtures/sidechain/`. Drop the `crates/gates` change: nothing in this track runs it. |
| G4 | Tests added at the end of shared test files clash at the merge. taste-next adds to `graph/tests.rs` (+109 lines), `tx/delta_tests.rs` (+20), `view_tests.rs` (+18) and `core/script/src/tests.rs`. | Put our tests in new files. `graph/overlay.rs` declares `#[cfg(test)] #[path = "overlay_tests.rs"] mod tests;`, and uses `super::tests`' helpers as `preview_tests.rs` does. Elsewhere, add a new file, or put the tests in the middle of the file. |
| G5 | The plan edits the `writes` check (`routes.rs:132`), 3 lines below taste's `WRITES` hunk (now `[&str; 22]`). | `routes_8b::router(state)` gets its own `route_layer` that refuses writes in a read-only window, with the same 423 answer. `routes.rs` then gets one line, `.merge(routes_8b::router(state.clone()))`. Put it just before `/api/project/templates`, away from taste's hunks, and before the `.layer` calls, so the token check covers it. |
| G6 | `timeline()` is shared: taste changes lines 663 and 696 around the lane loop. | Add the clip `sha` with one call in the rebuilt branch: `Arc::new(clips::with_sha(value(&lane), shown.as_deref()))`. It hashes `LaneData::lines_of(id)`. |
| G7 | taste-next moved `rename_label`'s options editing into `rename::name_edit`. | Don't copy that code into `mute.rs`. Write a small option edit over `crate::sites::properties` (already `pub`). At the merge, both can share one helper. |
| G8 | The plan says `view.rs` isn't touched by 8b.1 to 8b.3. taste-next does touch it (`ViewNode.title`, `locate`). | Our change (`InputPort.lane` in `InputPort::new`, and the lane edges near line 644) doesn't overlap. Expect a merge check, no conflict. |
| G9 | `m2 automate --remove` edits the `automate` arm (`main.rs:552`), close to where taste adds its arms. | A new command, `m2 unautomate <project> <track>/<lane> [--at <bar.beat>]`, in `apps/m2/src/edits.rs`. |
| G10 | `ci/check.sh` writes to `$HOME/m2-check/<sha>` by default. | At the End: `M2_CHECK_OUT=/home/jerry/m2-runs/be-scratch/check`. |

### R. Real-time and determinism

| # | Risk | Decision |
|---|---|---|
| R1 | At value 0 the op gives `x × 0`. A muted track that makes NaN or infinity (a plugin, a broken kernel) still makes the output NaN (`inf × 0`). A strict export then fails, for a track the user muted in the song. | At `cur == target == 0`, write `+0.0`. The glide still multiplies. With A9 the gate holds. No render record changes: the recorded fixtures mute nothing. |
| R2 | A bad slot, pitch or velocity in `Live` must never panic on the audio thread (a panic there crashes playback). | The server checks: a notes lane of the plan, a finite pitch in 0 to 127, a velocity in 0 to 1. The engine looks the slot up with `get_mut`, and refuses what doesn't fit. |
| R3 | A glide in flight when an activation lands ends as a step, of at most what is left of it. | Accept, as the design does. Say it in RENDER-22a. |
| R4 | Window-only overlays (A3) have no cell. A mute click during the 5 to 10 ms window doesn't reach them. | Accept. |
| R5 | `Player::live` goes through the bounded queue, under the control lock. `send` may wait up to 500 ms when the queue is full. | Accept: `swap_lane` and `activate` work the same way today. |
| R6 | Panic, reset and offline seek must leave overlays alone. | `reset_node` has no arm for `Overlay`; `cur` and `target` stay. Test it in `overlay.rs`. |
| R7 | Taps on the audio thread. | `fetch_max` on `AtomicU32` only. Offline renderers get no taps. The `TapSlots` leave with the retired renderer. |

### S. Scope, time and simpler shapes

| # | What | Proposal |
|---|---|---|
| S1 | The plan's sizes (3 L, 6 M, 3 S) add up to 6 to 7 agent-days, one slice after another. The owner wants the app in 2 days. | The owner picks (S1 in Decisions). Default: the audit's ranks, each feature usable as early as possible. B1, B2, B3, B5 (mute and solo, and the sidechain proof); B6, B7 (clips); B9, B10 (audition); B11, B12 (meters); then B4 and B8. Stop where the time ends; what's left goes in `docs/gaps.md`. |
| S2 | B6 to B8 share no file with B9 to B12, apart from `routes_8b.rs`, `edits.rs` and the build notes. | They could run as a second agent, in a second worktree branched from `8b-backend`, merged back. That needs the orchestrator. Default: no. |
| S3 | `Flat.side_inputs` as a public field. A3 shows the old `Flat`'s site list is enough. | The checker uses the sidechain inputs inside `check()` only. One field less, and PKG-13's edit gets shorter. |
| S4 | `set_taps`, the `Tap` enum and the server choosing taps. | A live renderer (`prepare`) taps every celled overlay node and the output when it is built. `PlanInfo.taps: Arc<TapSlots>`, with each tap's track. No new API; offline renderers have none. Post-fader meters come with 8b.3 either way. B11 and B12 drop to S. |
| S5 | "An activation that replaces a lane's consumer clears that lane's live table." | Drop it. A replaced consumer becomes a ghost, and is released at entry (`keep.rs:223`), so its live note ends anyway. A later Off for an id the new consumer never saw is ignored by poly (`lib.rs:1440`) and by plugins (`lib.rs:606`). Keep the test; it checks that. |
| S6 | `PlaysView` gains three fields (lanes, sites, taps). | One field, `info: Arc<PlanInfo>`, which holds all three and the cell slots. Fill it in both `PlaysView` literals (`actor.rs:890`, `:1040`). |
| S7 | W2 waits for `layout.rs` on main. taste-next already has `layout.rs` (merged at `d5a4a1f`). | Hand markers to taste-next now. W1 runs only if std 0.2 reaches main before the End; otherwise the third gate clause stays open in `docs/gaps.md`. |

### Revised slices

| # | Change from §6 | Size |
|---|---|---|
| B1 | IR, checker (`Flat.overlays` with `Feedback`, `Flat.muted`; refuse `muted` on `song` and on non-tracks), the delta's `Muted`. No SDK option yet, no `side_inputs`. | S |
| B2 | The op, cells in their own table, `set_overlays`, A3's one rule, A4's order; the SDK option and 1.2.0; the tone and the click test in `core/play/tests/mute.rs`; the renders check. | L |
| B3 | As planned, with A1, A6, M2, M6, G5. | M |
| B4 | As planned, with G7. | M |
| B5 | The ducking gate in `core/play/tests/mute.rs` (G3, A9). No `crates/gates` change. | S |
| B6, B7 | As planned, with A8, G6. | L, M |
| B8 | As planned, with M7, G9. | M |
| B9 | As planned, with A10, M4, R2, S5. | M |
| B10 | As planned, with S6. | M |
| B11 | Fixed taps (S4) and the callback timer. | S |
| B12 | Meters on a `watch` channel (M1), M3. | S |

## Decisions

"Decided" means build it this way now. "Owner, default" means build the default now, and the owner may change it.

| # | Question or change | Build with | Who |
|---|---|---|---|
| Q1 | 8b.4, 8b.6 and 8b.8 before 8a? | Yes. This track was given them; the runtime runs on one thread today. | Decided |
| Q2 | Where track meters tap before 8b.3 | At the track's overlay nodes (after mute, before the fader). Post-fader comes with the 8b.3 merge. | Decided |
| Q3 | The DSP load without 8a | A callback timer in `core/play`. Read only by the meters thread (M3). | Decided |
| Q4 | Keep `M2_AUDIO=paced`? | No. `none` already plays in real time. | Decided |
| Q5 | When the keyed compressor lands | W1, only after std 0.2 is on main (S7). | Decided |
| Q6 | `feeds` for a fed node in `song` | Name `song` and the component until `mix.rs` is merged. | Decided |
| Q7 | Overlay entries when a track is deleted or added | `Graph::forget_overlays(keys)` now. taste's routes call it at the merge. | Decided |
| Q8 | How a gesture says which clip it saw | A `sha` per clip, added in `timeline()` by one call (G6). The CLI sends none. | Decided |
| Q9 | Delete `/api/graph/lane` now? | No. It goes when the roll sends `Notes` gestures. | Decided |
| Q10 | How Remove automation is named and valued | By (module, lane), at the route's beat; the CLI's `--at`, else the fallback. CLI: `m2 unautomate` (G9). | Decided |
| Q11 | Markers here or in taste-next? | In taste-next, now (S7). | Owner, default |
| Q12 | Audition over the WebSocket only? | Both: the WebSocket, and `POST /api/graph/audition`, through one handler. | Decided |
| Q13 | A new event type for overlay changes? | No: the usual `graph` event. The route's answer carries the overlay state. | Decided |
| Q14 | Mute in song offline: a `prepare` argument or the graph? | The graph: `Flat.muted`. | Decided |
| Q15 | Fades and ghosts reading a muted track | Through an overlay, on both sides of a fade (A3). | Decided |
| Q16 | `@m2/core`'s version | 1.2.0, in B2 (G2). Take the higher one at a merge. | Decided |
| Q17 | Live notes on stop, seek and wrap | They keep sounding. | Decided |
| Q18 | The audition latency gate (G13) | At the End, if time allows. Linux numbers are information only. | Decided |
| A1 | `structural()` and `Muted` | Leave `structural()` alone; decide in `reach_to`. | Decided |
| A2 | Overlay cells | Their own table in `Cells`; `CellAt` unchanged. | Decided |
| A3 | Overlays in the activation window | One rule over the window graph, from both `Flat`s' site lists. | Decided |
| A4 | An overlay's place in the order | Right after its source; window-only ones not in `order_after`. | Decided |
| A5 | `feedback` consumers | `Consumer::Feedback { node }`. | Decided |
| A6 | The renderer built at open | Gets the overlays from the file before it loads. | Decided |
| A7 | What `set_overlays` writes | The table and the node only, never the slots. | Decided |
| A8 | Splitting pitched or reversed audio clips | Refused, in words. Exact-sample test only on a sample line. | Decided |
| A9 | How the gate compares | `f32` equality, not bits. | Decided |
| A10 | Live notes at panic | Cleared without Offs. | Decided |
| M1 | How meters reach windows | A `watch` channel, not the broadcast. | Decided |
| M2 | Writing `overlays.json` | Marked changed; written by `music2-tell`, temp file then rename. | Decided |
| M3 | Who reads the load | The meters thread only. | Decided |
| M4 | `Live` while held for an export | Refused and counted. | Decided |
| M5 | `muted` on a module that isn't a track | Refused by the checker. | Decided |
| M6 | Where overlays live in the server | In `Published`, with the accepted `muted` list. | Decided |
| M7 | Lane names (D6) | `<entry>-<control>` for a dictionary slot; else the slot, with the binding in front only on a clash. Update `e2e/s6-builders.mjs:159`. | Decided |
| M8 | `overlays.active` in the codes table | Added in B3. | Decided |
| G1 | Checks per slice | Plus `cargo check --workspace --all-targets` and clippy on the touched crates; B2 also the offline renders. | Decided |
| G2 | The SDK option's slice | B2, not B1. | Decided |
| G3 | Where the click and ducking tests live | `core/play/tests/mute.rs`. No `crates/gates` change. | Decided |
| G4 | Where new tests go | New files, never the end of a shared test file. | Decided |
| G5 | Routes and the read-only check | `routes_8b.rs` with its own `route_layer`; one `.merge` line in `routes.rs`. | Decided |
| G6 | The clip `sha` in `timeline()` | One call in the rebuilt branch. | Decided |
| G7 | The track option edit | A helper over `sites::properties`; nothing copied from `rename.rs`. | Decided |
| G9 | Remove automation's CLI | `m2 unautomate`, in `edits.rs`. | Decided |
| G10 | The End check's output folder | `M2_CHECK_OUT=/home/jerry/m2-runs/be-scratch/check`. | Decided |
| R1 | The op at value 0 | Writes `+0.0`. | Decided |
| R2 | Bad `Live` input | Checked by the server; the engine refuses, never panics. | Decided |
| R3 | A glide cut by an activation | A step of what is left. Said in RENDER-22a. | Decided |
| S1 | What fits in 2 days, and in what order | B1, B2, B3, B5, B6, B7, B9, B10, B11, B12, B4, B8. Stop where the time ends; the rest goes in `docs/gaps.md`. | Owner, default |
| S2 | A second worktree for B6 to B8 | No: one worktree, one slice at a time. | Owner, default |
| S3 | `Flat.side_inputs` | Not a public field; used inside the checker only. | Decided |
| S4 | How taps are chosen | Fixed when a live renderer is built: every celled overlay and the output. No `set_taps`, no `Tap` enum. | Decided |
| S5 | Clearing a lane's live table at an activation | Dropped; the ghost's release ends the note. | Decided |
| S6 | What `PlaysView` gains | `info: Arc<PlanInfo>`. | Decided |
| S7 | W1 and W2 | W2 to taste-next now. W1 only if std 0.2 is on main before the End. | Owner, default |
