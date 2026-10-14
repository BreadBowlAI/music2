# Step 8b, mapped against today's code

A survey for the step 8b design, written 2026-10-14 against `docs/architecture.md` (normative) at HEAD
`cbf5eb9`, with the step 7 design (`.git/m2-step7/design-7.md`, revision 2) as the engine 8b will sit
on. Read-only: nothing was built or run, and no cargo command was used in the repo. Another workflow is
editing the working tree (the G11 data-plane work: `core/session/src/{tx/mod.rs,take.rs,texts.rs,…}`,
`core/song/src/{lane.rs,lane/,data.rs,hash.rs}`, `crates/editor/src/notes.rs`,
`crates/server/src/graph/actor.rs`, `crates/gates/src/server.rs`), so every file is quoted as HEAD has it
(`git show HEAD:…`). The screenshots cited are the shared UX audit's, in
`scratchpad/ux8/shots/` (taken from the HEAD worktree by another agent); none were taken for this note.

Step 8b's line in the order of work:

> **8b:** each a Tx builder or an engine feature on this base: add, duplicate, delete and rename track;
> the mixer with monitoring mute and solo plus the `muted` track option; automate a knob; the tempo bar,
> with its lease; clip editing by id, with lane leases; audition.

Its gates (step 8's row, the parts that aren't 8a):

> Each producer feature lands without changing core's write API. Muting the kick in a fixture still
> ducks the bass, with the ducking read from `kick.env`, from `follow(kick.out)` in `song.ts`, and through
> a compressor keyed from the kick's audio. Toggling mute while a sine plays makes no sample-to-sample
> step larger than twice the sine's own largest. An export is byte-identical with and without overlays
> active.

Plus the CI gate from step 8: "overlays never change an export" (MON-4).

---

## 0. The short version

| 8b item | At HEAD | Kind of work | Rules | gaps.md | Waits on step 7? | The hardest open question |
|---|---|---|---|---|---|---|
| Add track | Not built. Tracks are hand-written; the code editor can't create files | Tx builder + UI (instrument picker) + std metadata | WRITE-10 row, ID-2, ID-5, ID-18, FMT-11, FMT-19, HOME-3 | #1, #32, #37 | No (structural edits work today; tails of other tracks are cut until the arena) | Where in `song.ts` it goes when the song has no std `mixer`, and how the binding name is chosen |
| Duplicate track | Not built | Tx builder | ID-2, FMT-12 (copies mint ids), FMT-17 (state with sidecar), ID-18 | #1 (implicitly), #9 | No | What "the channel strip" is when the mixer entry is an expression (`duck(bass.out, kick.env, …)`) |
| Delete track | Not built | Tx builder | WRITE-3 (Remove), WRITE-15/21 (before bytes, undo) | #10 | No | Other code reading the track (`kick.env` in a `duck`): refuse, or cascade |
| Rename track | Label rename **built** (step 6); key rename, Restore **not built** | Tx builder (key) | ID-10, ID-17, ID-18, DIAG-6 (Restore) | #10 | Yes, to keep the track's state: step 7 matches never across module keys (design-7 §3.4) | Whether to rename `tracks/<key>.ts` too; re-keying `layout.json` and `.music2/overlays.json` |
| Mixer panel | No panel. std `mixer` has optional `db`/`pan` per channel and no mute/solo; channels without `db` written have no fader at all | UI + editor (channel lookup, insertion into dictionary entries) + std (master fader, maybe `db` not optional) | ID-18, PKG-5, PKG-6, HOME table | #4, #20, #21, #27 | No | Faders for channels whose `db` isn't written (kick and bass in first-song) |
| Monitoring mute and solo | Not built. Sidechain ports are recorded (`Flat.sidechain`), nothing reads them | Engine feature (overlay cells in every plan) + control plane + `.music2/overlays.json` + UI | MON-1–4, PKG-13, RENDER-22a, RENDER-23, RENDER-2, DIAG `overlays.active` | #4 | Best built on step 7's `PlanInfo` | Where the overlay gain lives in the plan (a new built-in op) and how solo's path rule is computed |
| `muted` track option | Not built. `ModuleChange::Muted` exists in the Tx type, inert; no `Module.muted`, no SDK option | SDK + IR + delta + engine (same cells) + Tx builder | MON-5, ID-11, WRITE-8, WRITE-10 row | #4 | No | `GraphDelta::structural()` counts every module change, so a mute toggle would rebuild |
| Automate a knob | **Built** in step 6 (`automate.rs`, route, `m2 automate`, knob menu) | What's left: drawing points, override latches, stop automating | HOME-2, MON-6, WRITE-22a | #14 | No | MON-6 latches need ControlIds; one that lasts the session is enough for 8b |
| Tempo bar and its lease | Shown only ("4/4 · 116 BPM" in the timeline header, marks on the ruler). No writer, no route. A tempo change rebuilds the renderer | Tx builder (Write `data/arrangement.json`) + lease (control plane) + engine retime + UI | FMT-13, RENDER-27, RENDER-27a, WRITE-22, HOST-6 | #6, #37 | Yes: step 7 makes a tempo change an identity `Activate`; the lease needs that | "Each update reaches notes … at the next block" against an `Activate` per update; key, scale and title have no home in the contract |
| Clip editing by id | Notes lanes only, whole lane sent per edit, diffed into a `Clips` op. Timeline: add a 1-bar clip, move along its own lane in whole beats, delete. Automation and audio lanes are read-only in the app | Tx builder (clips by id, any lane kind, across lanes) + UI | FMT-11, FMT-12, WRITE-3 (`Clips`), WRITE-12 (exception to close), WRITE-21 | #9, #14, #15, #18 | No | Audio clip edits need prepared renders: no cheap preview |
| Lane leases | Not built. A note edit is heard once its Tx is accepted (p50 p95 26.6–34.6 ms) | Control plane + preview `SwapLane` | WRITE-22a, RENDER-26, RENDER-26a, RENDER-29 | #8, #9, #14 | Best built on `PlanInfo` | Previews against outside writes and new renderers, as knob leases already handle |
| Audition | Not built. `Event` is reserved whole, no `Live` message; `Player::panic()` exists with no route or button | Engine feature (`Live(Event)` merged into a lane's buffer) + route + piano roll | MON-7, RENDER-14, RENDER-6a, RENDER-28 | #8, #24 | Engine-wide note ids come with step 7 (§8.1) | What stop, seek and a replaced consumer do to a held live note |
| (Loop and transport) | **Built**: play, stop, toggle, restart, seek, loop range and toggle, in the UI and the API | Small leftovers | RENDER-9, RENDER-9a, HOME table (loop range is local) | #7, #19 | No | gaps.md still lists #5 and #7 as missing |

Three things cut across all of it:

1. **Core is not finished for 8b, but its write API is.** Every 8b write is expressible with today's ops
   and expectations (§1.2). The core changes 8b needs are in the IR (`Module.muted`), the SDK
   (`TrackOptions.muted`), the delta (`structural()` must not count `Muted`), the runtime (an overlay
   op, `Live(Event)`), and the view's `Home` (no value for the arrangement). WRITE-9 says each of these
   changes the contract first.
2. **Three new previews** (lane leases, the tempo lease, audition) and one new overlay all live on the
   control plane, beside knob leases. Step 7 replaces `Plays.{slots, caps, cell_slots, cells}` with
   `PlanInfo` (design-7 §14), so they should be built against step 7's shape, not HEAD's.
3. **`layout.json` has no reader, no watcher, and `plan::base_of` can't base it** (it isn't in a
   snapshot, so its base always comes out `Absent`). Track colour and order, and a new track's colour,
   need all three (§2.6).

---

## 1. What every 8b feature stands on

### 1.1 The builder pattern (`crates/editor`)

Every gesture is a function `(&mut Session, at, request) -> Result<Edited, EditError>` that plans a Tx on
the accepted revision, checks it, asserts what the gesture means on the checked candidate, and commits
(`crates/editor/src/plan.rs`):

- `plan::planned(s, label, ops, expect)`: a Tx with a random id (FMT-11) and every touched file based on
  the accepted revision (`base_of`: the text it was planned on, a binary's hash, a folder's directory
  hash, or `Absent`).
- `plan::run_as(s, tx, said, assert)`: `check`, the planner's assertion on the candidate, `commit`; a
  broken `Confined` is reported in the gesture's own words.
- `plan::unmoved`: a structural planner refuses when the file it planned in changed on disk since
  (`EditError::Stale`).
- `ready(s)`: a gesture that writes code needs the draft to compile (the draft rule).

The gestures today (`Gestures` in `crates/editor/src/lib.rs`): `set` (Values), `connect`, `disconnect`,
`add_effect`, `remove_effect` (Confined), `write_notes` (Confined lanes), `relink`, `delete_data`,
`apply_fix`, `rename_label` (Identical), `automate` (Confined), plus `data::plugin_state` and
`data::place_clip` for the CLI. Helpers 8b can reuse:

| Helper | Where | What 8b uses it for |
|---|---|---|
| `rename::track_call(sites, files, m)` | `rename.rs` | The `track()` call that made a module, only when it's in a project file, written with the key as a string literal, and unique. Add `muted`, rename the key, duplicate, delete |
| `automate::module_call` | `automate.rs` | The same, or the root's `song()` call |
| `sites::properties(text, span)` | `sites.rs` | Edit an options object (`name`, `muted`); a mixer's channel object |
| `sites::fn_params`, `imported_names`, `import_insertion` | `sites.rs` | Imports. `import_insertion` adds a *named* import from a module; a default import of `./tracks/<k>` is new |
| `keys::propose(label, taken)` | `keys.rs` | ID-2's one key proposer: a new track's key, a duplicate's key |
| `Snapshot::taken(path)` | `core/song` | A path taken in any capitals (`data/<key>/`, `tracks/<key>.ts`) |
| `ClipId::derive`, `tx::random_id` | `core/song/src/key.rs`, `tx.rs` | Clip ids (FMT-11) |
| `m2_song::canonical`, `Arrangement::canonical` | `core/song` | Lane files and the arrangement in canonical form (FMT-8a) |
| `notes::describe`, `notes::place` | `notes.rs` | Labels for history: "clip at bar 10 (1 bar)", "bar 2 beat 3" |
| `effect::effect_at`, `controls::component_of` | `effect.rs`, `controls.rs` | Recognise a component by its frame's ComponentId (PKG-9), so the mixer is `std:mixer` whatever it's imported as |

Every builder is reached three ways: a route in `crates/server/src/routes.rs` (listed in `WRITES`, so a
read-only window answers 423), a CLI command in `apps/m2/src/main.rs` (which takes the lock), and the
generic `POST /api/tx/check|commit` and `m2 tx`.

### 1.2 What core's Tx already gives 8b

From `core/song/src/tx.rs` and `core/session/src/tx/` at HEAD:

- **Ops:** `TextEdit`, `Write` (text or base64, or `Payload::Local`), `Remove`, `Rename` (a file, or a
  folder `data/<module>/` on its directory hash), `Clips { lane, put, delete }` (at most one per lane).
  A `Remove` under `data/` also removes the folders it leaves empty, up to `data/`
  (`core/session/src/tx/stage.rs`). A binary target's before bytes are journaled once, by content
  (WRITE-15), so deleting a track with a 50 MB plugin state is undoable.
- **Expectations:** `Identical`, `Values`, `Confined { ranges (a file's ranges, or "whole"), lanes,
  modules }`, `Seen`. `Confined`'s five checks (`core/session/src/tx/expect.rs`): nodes that differ were
  made inside a declared range (or only rewired to such nodes); lane data differs only for declared
  lanes; modules change *exactly* as declared; every edited byte is inside a declared range; a state,
  an asset or the arrangement changes only where the Tx's own ops wrote its file.
- **Module changes:** the delta computes `Added`, `Removed` and `Rekeyed` (a removed and an added module
  whose graphs are equal but for the key, `modules()` in `delta.rs`). `Muted { module, muted }` is in the
  type and the Tx JSON, "inert until `Module.muted` exists" (`tx.rs:194`).
- **`layout.json`:** a Tx may write it; a Tx that writes only it makes no revision but is still one undo
  step (`core/session/src/tx/mod.rs`, "Only layout.json written: no revision (PLANE-3)").

So every 8b write fits without a new op or expectation:

| Gesture | Ops | Expectation |
|---|---|---|
| Add track | `Write tracks/<k>.ts` (base `Absent`), `TextEdit song.ts` (import, mixer channel), optionally `Write data/<k>/<lane>.json`, `Write layout.json` | `Confined { tracks/<k>.ts: whole, song.ts: ranges, lanes: [new], modules: [Added(k)] }` (WRITE-10's row) |
| Duplicate track | as add, with the copied file text, copied lane files (new clip ids), copied state blobs and sidecars | `Confined { …, modules: [Added(k2)] }` |
| Delete track | `Remove tracks/<k>.ts` (or a `TextEdit` when the track is written in `song.ts`), `TextEdit song.ts`, `Remove` each `data/<k>/…` lane and state file | `Confined { …, modules: [Removed(k)] }` |
| Rename key | `TextEdit` of the key string (and the channel key when it equals it, ID-18), `Rename data/<old>/ → data/<new>/`, `Write layout.json` | `Confined { ranges, modules: [Rekeyed{old,new}] }` |
| Mute in song | `TextEdit` of the track call's options | `Confined { the options' range, modules: [Muted] }` (WRITE-10's row) |
| Tempo, meter | `Write data/arrangement.json` | `Confined {}`: check 5 admits an arrangement the Tx writes. WRITE-10 has no row for it |
| Clips by id | one `Clips` per lane (two for a move across lanes) | `Confined { lanes }` |
| Reorder, recolour | `Write layout.json` only | anything: no revision, so no delta (WRITE-10 has no row) |

To verify in the design, not assumed here: that adding a channel to `mixer({…})` changes only nodes
`Confined` attributes to the object literal's range (the mixer's `sum` kernel is made inside std's
body, a package site whose frame chain starts at the `mixer` call in `song.ts`; `add_effect` passes
the same kind of check today), and which lanes the delta lists for a removed or re-keyed module.

### 1.3 How an accepted change reaches the engine

Today (`reach` and `way` in `crates/server/src/graph/actor.rs`): the delta from what plays to the
accepted revision decides. Values alone → cell writes (`value_writes`, when `same_layout`); notes and
control lanes alone, each fitting its scheduler → `SwapLane`; **anything else** (structural, a module
change, a plugin state, an asset, the arrangement, an audio lane) → a new renderer, crossfaded, carrying
plugin instances, with every kernel starting from init.

After step 7 (design-7 §8.3): the same, except the "anything else" row becomes an `Activate` that keeps
every instance whose identity and shape match: an identity activation for data-only changes (audio
lanes, assets, plugin state, the arrangement), `continues` for code. No `SetArrangement`, no
`LoadState` message; `Hold(bool)` is new for export.

What that means for each 8b feature:

| Feature | Delta | Path today | Path after step 7 |
|---|---|---|---|
| Add, duplicate, delete track | structural, `Added`/`Removed` | new renderer: every other track's kernel tails cut | `Activate` with `continues`: untouched tracks keep their tails; a deleted track's nodes ring out as ghosts for 5 ms |
| Rename key | structural (`Rekeyed`) | new renderer | `Activate`, but the renamed track's instances aren't matched (passes "never across modules"; design-7 §3.4 leaves "a step 8b key rename can add a pass over `ModuleChange::Rekeyed`") |
| Mute in song | `Muted` | new renderer, because `structural()` is true for any module change (`delta.rs:84`) | must become a cell write with a ramp (MON-5): `structural()` and `reach` change |
| Monitoring mute, solo | none (no revision) | n/a: control plane, cell writes | same |
| Fader, pan | `Values` | cell write | same |
| Tempo, meter | `arrangement` | new renderer | identity `Activate`: every notes lane re-prepared in the new map, every audio lane re-rendered (design-7 §8.4) |
| Notes and control clips | `lanes` | `SwapLane` | same |
| Audio clips | `lanes` (audio) | new renderer | identity `Activate` with a `LaneMove` |
| Audition | none | n/a | `Live(Event)` |

### 1.4 The control plane

Previews never touch the session (RENDER-29). Knob leases are the model (`crates/server/src/graph/lease.rs`,
WRITE-22): `begin` resolves once against what the session last published, `update` writes straight
into the cells of the renderer that plays (`Player::set_cells` by a `CellTarget`), `commit` is one Tx on
the actor, a renderer built while a lease lives gets its value before it's sent, a code change carries
the lease or ends it, and leases end on cancel, idle TTL (10 s, 2 s keepalive) or the window closing.
The transport (`Graph::seek`, `set_loop`, `transport`, `status`) uses a separate `Timing` (the accepted
tempo map, the rate, the loop) and never waits on an edit.

8b adds four things here, and each needs what the knob lease has: which renderer plays, its slots and
generation, and how to put the preview back into a renderer built later.

| New on the control plane | Sends | Needs from what plays |
|---|---|---|
| Lane lease (WRITE-22a) | preview `SwapLane(generation, slot, data)` | lane slot by `LaneAddr`, its scheduler's caps |
| Tempo lease (RENDER-27a) | a retime per update | the plan, to prepare an identity activation |
| Monitoring overlays (MON-1) | cell writes | each module's overlay cells (`CellId`s mapped from a module key, RENDER-23) |
| Audition (MON-7) | `Live(Event)` | lane slot by `LaneAddr` |

At HEAD these would read `Plays.slots`, `caps`, `cell_slots`, `cells`; after step 7 they read
`PlanInfo` (design-7 §5.3, §14).

### 1.5 What 8b may change in core

The gate says "without changing core's write API": `save`, `Tx`, its ops and expectations, `check`,
`commit`, undo. WRITE-9 adds that a change that adds a gesture and touches `core/*` changes the
contract first. 8b's core touches, none of them to the write API:

| Crate | Change | For |
|---|---|---|
| `core/script/sdk/core.ts` | `TrackOptions.muted?: boolean`, emitted into the IR (and allowed by `options()`, which today admits only `name`) | MON-5 |
| `core/ir` | `Module.muted` (ID-11 already spells `Module { key, label, muted }`) | MON-5 |
| `core/session/src/tx/delta.rs` | `structural()` excludes `ModuleChange::Muted`; the delta reports it | MON-5's "sets the cell, with a ramp and no rebuild" |
| `core/session/src/lib.rs` | `Home` has no value for the arrangement (`Code, Lane, State, Layout, Local`) | HOME-1 for the tempo bar |
| `core/runtime`, `core/ir` (Flat) | an overlay gain op at each consumer port of a track's audio export, its cells mapped by module key | MON-1, MON-2, MON-5 |
| `core/runtime`, `core/play` | `Msg::Live(Event)`, merged into a lane's event buffer | MON-7 |
| `std/` | instrument `editor` metadata; a master fader; maybe the mixer's `db` not optional; a compressor with an audio key input (for the gate) | add track, mixer, gate 8 |

---

## 2. Tracks

### 2.1 What a track is today

- **In code:** `track(key, { name }?, fn)` (`core/script/sdk/core.ts`). The key is checked against ID-1
  (`checkKey`), `song` is refused, and a second track with the same key is `key.duplicate` naming both
  places. The only option is `name`: `options()` refuses any other key (`unknown option "muted"`).
  `fn` gets `{ key, lane, time }`, never the label (ID-10). The track is registered when its `track()`
  call runs, which is when its file is imported, so **track order is import order** (`tracks` is a `Map`
  in insertion order, and `__build` emits `song` first, then the tracks).
- **In the IR:** `Module { key, label, track, lanes, nodes, exports, src, frames }` (`core/ir/src/lib.rs`).
  `src` is `file:line:column` of the track's function, which `track_call` uses to find the call. No
  `muted`.
- **In the timeline** (`crates/editor/src/timeline.rs`): every module that is a track or declares a
  lane, in IR order, each lane in declaration order. `web/src/components/SongTimeline.tsx` colours a
  track by its index among the tracks (`colorOf`), from a fixed palette of seven, so **adding a track in
  the middle recolours every track after it**. The header shows the label, "track" (or "mix"), the key
  when the label doesn't say it, and each lane's name and note count (see `ux8/shots/a07-…png`). No
  mute, solo, add or delete control.
- **Keys** (ID-1, ID-2, ID-9): checked, never normalised (`m2_ir::key`, `m2_song::Key`;
  `key.invalid` suggests a spelling). Proposed once from a label by `keys::propose` (fold to ASCII,
  lowercase, runs of other characters to `-`, 32 characters, `-2`, `-3`… while taken), today used only
  by automate for a lane key. Windows device names get a trailing `-` in paths (`data/aux-/`).
- **Labels** (ID-10): display only. `rename_label` (`crates/editor/src/rename.rs`) is one `Identical` Tx
  on the `track()` call's `name`: inserted, replaced, or removed when it equals the key. Reached by
  double-clicking the name (`web/src/components/TrackName.tsx`), `POST /api/graph/label`, `m2 label`.
  A lane's label (`lane.notes("hook", { name })`) has no rename gesture.
- **Data:** `data/<key>/<lane>.json`, plugin state at `data/<key>/<frames>/<plugin-key>.state` with its
  `.state.json`. A track key renamed by hand leaves the folder `data.unclaimed`, with Relink of the
  whole folder first ("Move all to …"), lane by lane after it (DIAG-6, `core/session/src/unclaimed.rs`).

### 2.2 Add track

**Contract.** WRITE-10's row: `Confined { a new file, ranges in song.ts, a new module }`. ID-2: a writer
proposes a key from a label once, shows it, writes it, never derives it again. ID-18: the app writes the
track key as the channel key of a new mixer channel. FMT-19 and HOME-3: colour goes in `layout.json`,
never in `track()` options. The deferred table puts "track templates" among the TypeScript package
tools, whose trigger hasn't arrived, so 8b's add track is a Rust builder in `crates/editor`.

**gaps.md #1:** "Pick an instrument (bass, keys, drums…) and music2 writes `tracks/<name>.ts`, imports it
in `song.ts`, adds a mixer channel and an empty notes lane, as one checked edit you can undo. Also swap a
track's instrument later." #32 adds installed CLAP plugins to the same picker; #37 asks for
`m2 add-track`.

**What the builder writes** (for "Lead Vox" with std `pluck`):

```ts
// tracks/lead-vox.ts                                     (Write, base Absent)
import { track } from "@m2/core";
import { pluck } from "std/instruments";

export default track("lead-vox", { name: "Lead Vox" }, ({ lane }) => pluck(lane.notes("notes")));
```

```ts
// song.ts                                                (TextEdit: two insertions)
import leadVox from "./tracks/lead-vox";
…
    mixer({
      …,
      "lead-vox": leadVox.out,                            // or { in: leadVox.out, db: db(0) }: see §3.2
    }),
```

**Decisions the design must make:**

1. **The instrument list.** The catalog (`Session::catalog`, `m2_script::Catalog`) has every
   component's descriptor, with its ports, so "has a notes input and an audio output" finds std's five
   instruments without core holding a list (P10, PKG-7). But std's instruments declare no `editor`
   field (`std/src/instruments.ts`: no category, no description), so the picker has nothing to group or
   explain them by. std needs `editor: { category: "Instruments" | "Drums", description }` on each.
   Plugins come from `m2 plugins`' list (gap #32), written as
   `plugin("clap:<id>", { notes: lane.notes("notes"), key: "<k>" })`.
2. **Where in `song.ts`.** The mixer is found by its frame's ComponentId `std:mixer` (PKG-9), never by
   the name `mixer`. If the song module has exactly one, a property goes at the end of its `channels`
   object. If it has none (a song that returns `tone.out`, the gate fixtures, a hand-written sum) or
   several, the builder must refuse, or wrap the song's output (`mixer({ main: <old>, "lead-vox": … })`,
   a structural rewrite). A track that is imported but whose export nothing reads still builds and
   shows in the timeline, but is silent: the builder must not leave one like that.
3. **The binding name** in `song.ts`. Not identity, but it must be a valid identifier that collides with
   nothing in scope: `leadVox` from `lead-vox`, `t808` from `808`, `masterTrack` when `master` is
   imported from `std/mix`. A new helper; there is none for default imports.
4. **The file name.** `tracks/<key>.ts`; if taken (in any capitals, `Snapshot::taken`), `tracks/<key>-2.ts`.
   File paths are never identity, so this is cosmetic.
5. **The key.** Proposed from the label, free among module keys *and* `data/<key>/` folders on disk in
   any capitals: an unclaimed folder left by a deleted track would otherwise be claimed silently
   (DIAG-3's `data.claimed` would say so after the fact). The dialog shows the key and lets it be edited
   (ID-2's "shows the key").
6. **An empty lane file or not.** A declared lane with no file plays empty and shows in the timeline,
   so the file isn't needed; writing it puts the lane in `Confined.lanes` and makes the folder visible
   to git and agents. gaps.md says "an empty notes lane".
7. **Colour and order.** A new track should get its colour written into `layout.json` in the same Tx,
   or the index-based palette shifts every later track's colour (§2.6).
8. **Where the track goes in the timeline**: the import's position in `song.ts` decides (import order),
   until `layout.json` order is read.
9. **"Swap a track's instrument later"** (gap #1): an instrument call replaced by another with the same
   notes argument. Like `add_effect`, a `Confined` edit of one call; not in 8b's list.

Today the app's code editor can't create a file at all (gap #34), so add track is the first way to get a
new file from the app.

### 2.3 Duplicate track

Nothing in the contract beyond the order of work, FMT-12 ("copying or duplicating a clip always mints a
new id") and ID-2. What it has to copy:

- **The code.** The track's file with the key string replaced and the label made "Bass 2", under a new
  key proposed from it. A track written in `song.ts` (the Empty template's way) is a copied call
  expression in the same file. A track made by a helper or a loop (several modules from one call, or a
  call not in a project file) can't be copied: `track_call` returns `None` and the builder refuses,
  saying so.
- **The data.** Every lane file, with every clip id re-minted (`ClipId::derive(tx.id, addr, n)` or
  random); plugin state blobs and their sidecars, byte for byte (`Write` with base64, or a
  `Payload::Local` staging copy for a big blob, so the Tx JSON doesn't carry 50 MB). The state path is
  `data/<module>/<frames>/<plugin-key>.state`, so only the module segment changes.
- **The channel.** ID-18 finds the channel by following the export into the mixer. When the channel
  is `{ in: bass.out, db: db(-10) }` the copy is the same object on the new binding. When it is an
  expression (`bass: duck(bass.out, kick.env, { depth: db(-9) })`), copying it with the binding
  substituted keeps the duck, which is what a producer expects of "duplicate the channel strip"; that
  substitution is new planner work.
- Exports other modules read (`kick.env`) are not duplicated into their readers.

### 2.4 Delete track

The code and graph editing design's rule for deleting a node applies: "Find all semantic references;
require replacements for required consumers or an explicit cascade" (`docs/code-graph-editing-design.md`,
Structural gestures). What the builder has to find:

- the `track()` call and its file (remove the file only when it holds nothing else);
- the import in `song.ts` (and in any other file that imports it);
- the mixer channel (by ID-18's edge);
- every other read of its exports: in first-song and the Starter beat, `kick.env` feeds `duck` on the
  bass (and chords). Deleting the kick must refuse with a message that names them ("Bass ducks to Kick:
  take the duck out first"), or remove those calls too as an explicit, listed cascade;
- its data: `Remove` each lane file and state file under `data/<key>/`; the folder goes when empty.
  Assets its audio clips play are left alone (FMT-14: never overwritten; nothing removes assets).
  Files under `data/<key>/` that aren't data (a `notes.txt`) keep the folder.

One undo step restores everything, binary states included (WRITE-15/21).

### 2.5 Rename track

- **Label:** built (§2.1).
- **Key** (ID-17): "one Tx that edits the code and renames `data/<key>/`. It may also write
  `layout.json`." ID-18: "A key rename Tx renames a channel key only when it equals the old track key,
  and says so in its label." Not built (ID-17's Today: "The app's key rename and Restore aren't
  built"). What else follows a key:
  - `layout.json` entries keyed by track key (colour, order): rewritten in the same Tx;
  - `.music2/overlays.json` (mute, solo) keyed by module key: local, rewritten by the server after
    the commit, outside the Tx (WRITE-3a refuses Local paths in a Tx);
  - plugin instances: HEAD's `CarryKey` includes the module key, and step 7 never matches across
    module keys, so **every instance on the renamed track restarts** (kernels from init, plugins
    re-created and their state reloaded) unless 8b adds the `Rekeyed` pass design-7 §3.4 mentions;
  - `tracks/<old>.ts`: renaming it to `tracks/<new>.ts` (a `Rename` plus the import path) is tidy but
    optional; nothing depends on it;
  - the binding name in `song.ts` stays (not identity).
- **Restore** (DIAG-6): the `data.unclaimed` fix that renames the key back. The same builder, run
  backwards; not built, and the strip offers only Relink and Delete.
- **Delta:** `Rekeyed` is detected only when the module's graph is otherwise equal (`modules()` in
  `delta.rs`): renaming a key and changing anything else in one edit reads as `Removed` + `Added`.

### 2.6 Reorder and recolour (gap #10; not in 8b's list, but every new track needs a colour)

FMT-19: `layout.json` holds track order and colour, keyed by track key, never part of a revision,
unknown keys ignored. HOME table example: `{"tracks": {"kick": {"colour": "#e4572e"}}}`. At HEAD:

- nothing reads or writes `layout.json` (FMT-19's Today);
- the watcher doesn't track it (`core/session/src/watch.rs`: "nothing else (PLANE-3)"), so an outside
  change to it is never noticed, and no event tells clients;
- `plan::base_of` reads the accepted snapshot, which never holds `layout.json`, so a builder that
  writes it would base it `Absent` and conflict whenever the file exists. A layout builder must hash
  the file from disk for its base;
- colours are by index (§2.1), so they move when tracks are added, deleted or reordered.

Pro producers rely on stable colours. The cheapest fix: the server reads `layout.json` into the view
(order and colour per track key, falling back to today's palette by index), add track writes the new
track's colour, and a light watch of that one file pushes a "layout" event.

---

## 3. The mixer, mute and solo

### 3.1 The mixer today

`std/src/mix.ts`:

- `mixer({ <key>: Audio | { in, db?, pan? } })`: one `each({ in, db, pan })` dictionary slot (PKG-6), so
  a channel's controls are `channels.<key>.db` and `channels.<key>.pan`. Both are `.optional()`
  (PKG-5): **a channel that doesn't write `db` has no gain node and no knob**, and one without `pan`
  no panner. `pan` is smooth; the `gain` kernel glides its own level (a 5 ms one-pole). Channels are
  summed in the caller's key order with `sum`. No channels is a silent sine.
- `master(x, { ceiling })`: a limiter at -1 dB by default. **No master fader.**
- No mute, solo, send, bus, meter or insert list.

In the fixtures: first-song's `kick: kick.out` and `bass: duck(bass.out, …)` write no `db`, so the Song
graph shows faders only for hats and keys (`ux8/shots/b02-first-song-graph-song.png`). The Starter
beat's `kick: kick.out` has none either. The p16/p50 references write `db` on every channel
(`core/session/src/reference.rs`).

The view already names the faders: a `Knob` carries `slot: "channels.hats.db"` and
`component: "std:mixer"` (`crates/editor/src/view.rs`), with `home: code`.

### 3.2 The mixer panel

**Channel ↔ track (ID-18):** "finds a track's channel by following the graph edge from the track's
export into the mixer, never by comparing keys." The edge may pass through other calls (`duck(bass.out,
…)`), so "following" means walking downstream from the export's node until a mixer channel's `in`.
No helper does this today; the cable planner's `refs` and the view's edges are the material. A track
that reaches no channel, or two, has no fader in the panel, and the strip says why.

**Faders where `db` isn't written.** The ways out:

1. **Make the mixer's `db` not optional**, default 0 dB. Then every channel has an inherited fader
   (PKG-3), shown at 0 dB in italics, whose first edit writes it in as a `Values` Tx (WRITE-6: "Inserting
   an omitted argument turns an inherited control into a written one, which must still count as
   `Values`"), through the existing lease path. Costs and checks:
   - renders: at a constant 0 dB the `gain` kernel looks bit-exact (`db_to_gain` in `core/kernel` is
     `10f32.powf(db / 20.0)`, and `powf(10, 0)` is exactly 1; the smoother primes to its target;
     `x * 1.0 == x`), so the fixtures' recorded renders shouldn't move and std's
     version needn't (FMT-3). To be proven by `core/session/tests/renders.json`, not assumed;
   - one more kernel instance (one more wasmtime `Store` after step 7, RENDER-32) per channel that
     didn't write `db`;
   - **the insertion planner refuses dictionary entries today**: `Insertion::plan` in
     `crates/editor/src/lens.rs` returns "it is a dictionary entry's, which is written with its
     entry". It has to learn to turn `kick: kick.out` into `kick: { in: kick.out, db: db(-3) }` and to
     add a property to an entry object.
2. **First touch is structural**: the first drag writes `db` (a `Confined` edit of the channel), and the
   drag continues as a lease once that revision plays (100–200 ms of a fader that doesn't move the
   sound).
3. **Add track and a one-time "tidy" write `db` on every channel.** Doesn't help songs written by hand.

`pan` stays optional: a mono input through the panner at centre is equal-power (`cos(π/4)·√2`), not
bit-exact, which is PKG-5's reason. So the pan knob of a channel without `pan` stays a first-touch
structural write (or the insertion above).

**A master fader** needs `master` to take a gain (std change, inherited 0 dB before the limiter).

**What else a strip shows:** the track's label and colour, mute and solo (overlays, §3.3), "mute in
song" (§3.4), and, per MON-2, that a muted kick still ducks the bass ("The mixer shows this"). Meters
aren't built: the deferred table's "Render taps, meters, track freezing, bounce as a Tx" has the
trigger "Mixer meters, or the resample loop in the UI", so **a mixer panel with meters is that
trigger**, and gap #20 notes a master peak is a small change in `core/play`. Sends and buses (#27) and a
sidechain picker (#21) aren't in 8b.

**Lanes of the song module** (a fader automated on the root module, `data/song/<lane>.json`) show on
the timeline's "mix" row; the panel has no channel for them.

### 3.3 Monitoring mute and solo

**Contract:**

- MON-1: engine overlays, "gain cells keyed by module key, on track modules", for every song, whatever
  its code. "The planner puts one overlay gain cell on each consumer port that a track's audio export
  feeds." The root module and parts within a track can't be muted (later, in `key()` frames).
- MON-2: mute silences the module's audio exports at every consumer port **except sidechain ports**
  (PKG-13), which always get the unmuted signal. Control exports (`kick.env`) keep flowing.
- MON-3: solo keeps every audio export on a path through a soloed module and mutes the others, with
  the same sidechain exception.
- MON-4: persisted in `.music2/` (HOME table: `.music2/overlays.json`); never changes files,
  revisions, exports or agent renders; an export made while overlays are active carries the Warning
  `overlays.active` (DIAG table).
- RENDER-22a: a `SetCell` on an overlay gain cell ramps linearly over one block. RENDER-23: overlay
  `CellId`s are mapped from a module key, and never outlive their plan. RENDER-2: "every sum (buses,
  `poly` voices, overlay mixes) runs in plan order in the consuming partition" (8a). Planes table:
  overlays are the control plane, "never the session".

**What exists at HEAD:**

- `Frame.sidechain` records what each sidechain input received; the checker carries each into
  `Flat.sidechain` (or a voice's) as a `PortRef` (`core/ir/src/check.rs`); std marks `follow`'s `in`
  and `duck`'s `side` (`std/src/dsp.ts`, `std/src/fx.ts`). "Nothing reads them until mute and solo
  (step 8)" (PKG-13's Today).
- `FNode.module` names each flat node's module, so an edge whose source is a track's node and whose
  consumer is in another module is findable in `Flat`.
- Ramped cells (`Cells::apply` in `core/runtime/src/cells.rs`), for controls declared smooth.
- Nothing else: no overlay node, no cells keyed by module, no file, no route, no UI.

**What to build:**

1. **An overlay op in every plan, offline included**, at each non-sidechain consumer port of each
   track module's audio exports. It has to be there whether or not anything is muted, so that a
   toggle never changes the plan's cell layout (`same_layout`) and is a cell write; and offline, so that
   "mute in song" (§3.4) applies there with the same cells. At gain 1.0 it must be bit-exact (`x * 1.0`),
   so every pure fixture still renders the same bits and RENDER-33's live-equals-offline test holds.
   It's a built-in runtime op, not a std kernel: core has no DSP (song-as-program), and the planner
   can't depend on a package. Where it goes is a design choice: inserted by the checker into `Flat`
   (every reader of `Flat` then sees it, the delta included), or by the runtime when it builds a plan
   from `Flat` (invisible to the session).
2. **Solo's path rule**, computed off the audio thread from the module-level audio graph (edges
   between modules through non-sidechain ports): a module is heard when it is soloed, or upstream or
   downstream of a soloed module along those edges. That keeps a bus track fed by a soloed vocal, and
   a vocal feeding a soloed bus ("solo safe" falls out of the rule). The result is one value per module,
   written to every overlay cell of that module.
3. **The cell values:** `muted_in_song ? 0 : (heard ? 1 : 0)`, ramped over one block.
4. **Persistence:** `.music2/overlays.json`, read on open, written by the server (not a Tx, not
   undoable, never in a revision). A renderer built while overlays are active gets their values in its
   cells before it's sent, as a knob lease's value does.
5. **A read-only window** (WRITE-17): the lock holder owns `.music2/`; a second window should keep its
   overlays in memory only.
6. **Export:** reads committed files only (RENDER-3), so overlays can't reach it; it adds
   `overlays.active` to what it reports while any is on. Agent renders and `m2 render` never read the
   file.
7. **HOME-1:** mute and solo are `home: local`.

### 3.4 The `muted` track option ("mute in song")

MON-5: saved in code, applies to every render, export included. `track("kick", { muted: true }, fn)`,
carried as `Module.muted`; the planner applies it with the same cells and the same sidechain exception
as a monitoring mute. Toggling it is a `Confined` Tx on the track call's options; "the delta's `muted`
change sets the cell, with a ramp and no rebuild". WRITE-8 already lists modules "muted" among the
delta's changes and says "a module's `muted` → its mute-in-song cell". At HEAD none of it exists past the
Tx type:

| Piece | HEAD | Needed |
|---|---|---|
| SDK | `TrackOptions = { name? }`; `options()` refuses `muted` | `muted?: boolean`; `@m2/core` 1.x minor (an addition, FMT-2) |
| IR | `Module` has no `muted` | `Module.muted` (ID-11) |
| Delta | `ModuleChange::Muted` in the type, never produced; `structural()` true for any module change | produce it; leave it out of `structural()`; let `reach` turn it into cell writes |
| Builder | none | the `track()` call's options edited like `rename_label` edits `name` (insert `{ muted: true }`, or remove it), `Confined { that range, modules: [Muted] }` |
| Engine | none | the overlay cells of §3.3 |

### 3.5 Gate 8's mute tests and what they need first

- "Muting the kick in a fixture still ducks the bass … from `kick.env`": first-song and the Starter beat
  duck from `kick.env`, a control export, so it flows regardless.
- "… from `follow(kick.out)` in `song.ts`": no fixture writes that; `follow`'s `in` is a sidechain port,
  so a test song, or a fixture edit, is needed.
- "… through a compressor keyed from the kick's audio": **std has no compressor with a key input**
  (`compress` takes only `in`), and a hosted plugin can't be keyed either: the host connects only a
  plugin's main audio input (`read_info` in `core/plugins/src/inprocess.rs`, `plugin_sig` in
  `core/runtime/src/packages.rs`), so PKG-13's "a hosted plugin's audio inputs other than its main
  one are sidechain ports" has nothing to apply to yet. So the gate needs a std change, such as
  `compress(x, { key: kick.out })` with `port.audio({ sidechain: true })`, which changes no existing
  render if the key is optional.
- "Toggling mute while a sine plays makes no step larger than twice the sine's own": the gates' sine is
  a full-scale 440 Hz `song()` with no track (`crates/gates/src/songs.rs`, `sine`), and there is
  nothing to mute without a track, so it needs a track-wrapped variant. A one-block (128-frame) linear
  ramp adds at most 1/128 of full scale per sample, against the 440 Hz sine's own step of about
  2π·440/48000 ≈ 0.058, so it passes. For a 50 Hz bass the sine's own step is about 0.0065, and the ramp's
  0.0078 is more than that: the gate as written can't catch a click on a low note. Most DAWs fade a mute
  over 5–10 ms.
- "An export is byte-identical with and without overlays active": true by construction if export never
  reads `.music2/overlays.json`; the test keeps it so.

### 3.6 Two mutes: a UX hazard to design for

Owner decision 4 gives music2 two mutes: a listening-only one (never exported) and "mute in song"
(saved, exported). In Logic, Ableton and FL Studio a muted track is left out of a bounce; here it isn't.
A producer will mute a track, export, and hear it in the file. `overlays.active` on the export is the
contract's answer; the export dialog could also offer "Mute these 2 tracks in the song too" (a Tx per
track) before rendering, which keeps MON-4 intact. And because overlays persist in `.music2/`, a song
reopened next week can open with half its tracks soloed away: the top bar needs a clear "something is
muted or soloed" indicator with "clear all".

---

## 4. Automate a knob

Built in step 6 (`crates/editor/src/automate.rs`, `POST /api/graph/automate`, `m2 automate`, the knob's
right-click menu): one `Confined` Tx replaces the literal (or writes an inherited control) with
`lane.control("<key>", "<unit>", <old literal>)`, adds `lane` to the module function's parameters when
missing, and writes an empty lane file; renders are the same bits before and after (HOME-2). Refused for
a value with no unit, one shared by several tracks, or one whose call isn't inside its module's
function, and the view says why (`Knob.automate_refused`).

What 8b still owes around it:

| Missing | Rule | Notes |
|---|---|---|
| Drawing points on a control lane (gap #14) | WRITE-3 (`Clips`), WRITE-22a ("drawing automation") | `write_notes` refuses control lanes; clip editing by id (§6) covers it |
| A knob a lane drives: dragging it sets an override latch, "Re-enable" or "Write to code" | HOME-2, MON-6 | A latch keyed by ControlId lasts the session; one on a keyed plugin's parameter may persist in `.music2/`. Today such a knob is a cable into the control, with ✕ to disconnect, which writes the descriptor's default and loses the fallback |
| Stop automating | code-graph design's table | "Replace that use with an explicit sampled value; preserve a still-shared lane" |

---

## 5. Tempo, meter, the tempo bar and its lease

### 5.1 Today

- **Format** (FMT-13, `core/song/src/arrangement.rs`): `data/arrangement.json` holds `tempo: [{beat,
  bpm}]` (first at 0, 20–999 bpm, strictly increasing beats) and `meter: [{beat, beats, unit}]` (empty
  is 4/4; changes on bar lines of the meter before). Strict (`deny_unknown_fields`); an invalid one is
  `arrangement.invalid` and blocks acceptance. `Arrangement::canonical()` writes it. `TempoMap` is
  **piecewise constant**: no tempo ramps.
- **Shown:** the timeline's top-left corner says "4/4 · 116 BPM" (the first entries), and later changes
  are marks on the ruler (`SongTimeline.tsx`). The top bar has no tempo (`ux8/shots/a07-…png`).
- **Written:** by hand only. No builder, no route, no CLI command (gap #37 asks for `m2 tempo`).
- **Engine:** the tempo map is baked into each renderer when it is built (notes data in samples, audio
  clips placed at `seconds_at(start)`); `reach` returns `None` for any arrangement change, so a tempo
  edit builds a new renderer and every kernel tail is cut. Plugins get tempo, meter, bar and loop in
  their transport event (`transport_event` in `core/plugins/src/audio.rs`), so HOST-6 is mostly met.
  The server's `Timing` keeps the accepted map for seek and loop checks; "a tempo change that makes the
  loop too short turns it off".

### 5.2 After step 7

A tempo or meter change is an identity `Activate`: every kernel, poly, plugin and ring kept, no fades;
every notes lane gets new data in the new map; every audio lane is re-rendered when the tempo map
changed (`ClipKey` includes it, so on a tempo change that's every clip); control lanes are in beats and
move unchanged; the transport keeps its beat (design-7 §8.4). `SetArrangement` is gone from the
contract's message list.

### 5.3 The builder

A `Write` of `data/arrangement.json` from `Arrangement::canonical()`, based on the accepted text,
expectation `Confined {}` (check 5 admits the arrangement because the Tx writes it). Labels such as
"124 BPM → 128 BPM", "4/4 → 3/4 from bar 9". It refuses what FMT-13 refuses (a meter change off a bar
line, say), with the message the reader would give. WRITE-10's table has no row for it.

### 5.4 The lease (RENDER-27a)

"A tempo drag is a lease. Each update reaches notes, control lanes, time and plugins at the next block.
Warped audio switches when its render is ready … Release commits one Tx that writes
`data/arrangement.json`." Two ways to meet it:

- **A. An identity `Activate` per update.** Off the audio thread, the control plane prepares the plan
  with the new tempo map: notes data for every notes lane, no audio re-render (the old renders play on,
  as RENDER-27a allows). Activations are never coalesced (design-7 §7.5), so the control plane must keep
  one in flight and send only the latest value. The cost per update at p50 is building ~50 lanes' notes
  data: unmeasured, probably a few ms. It meets "next block" only loosely: the update is heard at the
  first block after its prepare, not the next block after the request.
- **B. Tempo as engine state.** Schedulers read the tempo map each block and convert beats to frames
  themselves, so a tempo update is a slot write like a cell. Strictly "next block", but `NotesData` is in
  samples today, and so are audio-clip placements: a larger runtime change, and every swap and
  reconcile rule of RENDER-26a would be in beats.

A is the one that fits step 7's design; the contract's "at the next block" would then read "at the
next block after its plan is prepared". Either way the server's `Timing` (seek, loop checks, the status
`beat`) must follow the lease's map while it lives, and a code or data revision accepted during the
drag must be prepared with the lease's map, as knob leases put their value into every new renderer.

### 5.5 Decisions for the tempo bar

- With several tempo entries, which one the top bar's BPM edits: the one at the playhead, or all of
  them scaled. Logic edits the tempo at the playhead and has a tempo track for the rest; the timeline's
  ruler marks are the natural place to drag a change.
- A meter change is a click, not a drag: no lease. Changing 4/4 to 3/4 at beat 0 re-bars every clip;
  FMT-13 only constrains later changes.
- Tap tempo, tempo ramps and a tempo lane are in neither document (§11).

### 5.6 Key, scale and the song's title (gap #6) have no home in the contract

- **Key and scale.** FMT-13's arrangement is strict, so a `key` field there is a format addition, which
  bumps `format` (FMT-2) and needs `m2 migrate` (FMT-3a, "after format 1"). The HOME table has no row
  for it. For a piano-roll scale guide only (gap #16), it is presentation: `layout.json`
  (`{"song": {"key": "A minor"}}`; unknown keys are ignored, no format bump). If generators (std/theory,
  std/gen) are to read it, it belongs in code. An owner decision.
- **The title** is `[project] name` in `music2.toml`, a Manifest file: editing it is a `TextEdit` with
  `Identical` (the graph doesn't see it), but it takes the code plane (a compile), and the folder name
  doesn't follow.

### 5.7 HOME-1 and the tempo

HOME-1: every value the view and leases return names its home, and `Home` has no value for
`data/arrangement.json` (`Code, Lane, State, Layout, Local`). The tempo bar's value needs one, and the
HOME table a row: a core change (`m2_session::Home`), not to the write API.

---

## 6. Clips, clip ids, clip editing by id, and lane leases

### 6.1 Today

- **Format** (FMT-10–12, `core/song/src/lane.rs`): a lane file is `{"clips": [...]}` (control lanes add
  `unit`). Notes clips `{id, start, length, notes: [{pitch, start, length, velocity}]}`, control clips
  `{id, start, length, points}`, audio clips `{id, start, length, asset, hash, offset, gain_db, fade_in,
  fade_out, reverse, pitch, warp}`. Ids are 8 lowercase Crockford characters; readers accept
  `^[0-9a-z]{1,16}$`; unique within a lane; a clip without one is anonymous (`clip.anonymous`,
  AssignClipIds); duplicates are `clip.duplicate-id` (RenumberClips). Canonical form: one clip and one
  note per line, sorted.
- **Who mints ids:** the web (`newClipId` in `web/src/lib/graph.ts`, random) and `notes::tidy` for any
  clip sent without a valid unique one (derived from the Tx id). CLI and fixes derive (FMT-11).
- **The write:** `POST /api/graph/lane {session, revision, module, lane, clips}` → `write_notes`: the
  client sends the whole notes lane, planned at a revision; the base is the lane file as the session
  remembers that revision (`Session::snapshot(at)`, the last 64). When every clip has its own id the
  builder turns the difference into one `Clips` op (put the clips whose canonical line changed, delete
  the ids gone), so an outside edit to another clip merges and one to the same clip conflicts by name;
  otherwise the lane is written whole. Notes lanes only: control and audio lanes are refused. This is
  WRITE-12's stated exception ("the piano roll sends no lane hash"). The G11 workflow is rewriting this
  function in the working tree right now (comparing clips instead of lines, reusing the accepted parse).
- **The timeline UI** (`SongTimeline.tsx`): double-click an empty spot in a notes lane for a one-bar clip;
  drag a clip along its own lane, in whole beats; select and Delete. Automation and audio clips are
  drawn but can't be edited. No zoom (fixed 24 px per beat), no snap choice, no copy, paste, resize,
  split or duplicate.
- **The piano roll** (`NoteView.tsx`): draw, move, resize notes on a snap grid (1/4 to 1/32 or off), a
  velocity strip, the clip's length in bars, ⌘A, ⌘D, arrows (⇧ for an octave or a bar), Delete, zoom.
  Each gesture writes the whole lane on pointer-up; nothing is heard during the drag.
- **Audio:** `data::place_clip` places a bounce (`m2 bounce --place`); nothing in the app.

### 6.2 What "by id" should change

- **Clip-level requests:** `{lane, put: [clip], delete: [id], base}` for any lane kind, rather than the
  whole lane. A big notes lane isn't sent per keystroke, two windows or an agent editing other clips of
  one lane merge (WRITE-3's `Clips` rule), and the client sends its base (the lane's hash, or its text),
  which closes WRITE-12's exception.
- **Across lanes:** moving a clip to another track's lane is one Tx with two `Clips` ops; the clip keeps
  its id unless the target lane has it (FMT-12). Across kinds (a notes clip onto an audio lane) is
  refused.
- **Copies mint ids:** duplicate (⌘D), paste, repeat ×N, split (two clips from one: the second gets a new
  id; notes crossing the cut are cut or kept in the first, a decision).
- **Resize and trim:** the right edge changes `length`; the left edge changes `start` and, to keep notes
  in place, every note's `start` (an audio clip's `offset`). Notes outside the clip are kept but
  silent, as playback cuts them to the clip (the timeline's `audible` draws them that way).
- **Undo** of a clip edit after an outside change to that clip is a `Conflict` naming the lane and the
  clip (WRITE-21; built).
- **Audio clips:** every edit needs a render (RENDER-27): after step 7 an identity `Activate` with a
  `LaneMove`; until then a new renderer.

### 6.3 Lane leases (WRITE-22a)

"A lane edit made while playing (dragging a note, drawing automation) uses a lane lease:
`begin(lane, clip)`; `update` sends a preview `SwapLane` straight to the engine with no file write,
under the sounding-note rules of RENDER-26a; `commit` is one `Clips` Tx; `cancel` swaps the accepted
data back." Today: not built ("a notes edit is heard only once its write is accepted and the renderer
rebuilt"; since step 6, a `SwapLane` after the Tx, p95 26.6–34.6 ms at p50 per PLANE-5).

What a lane lease must do, by analogy with the knob lease:

- **Begin:** resolve `(LaneAddr, clip id)` against the accepted revision, and the slot that plays it.
- **Update:** build the lane's data off the audio thread with the clip replaced (`notes_data` for notes,
  `data.control` for control), check it fits the scheduler's caps (else the preview waits for the
  commit), and send `SwapLane(generation, slot, data)` through the player, not the actor. The
  scheduler's swap (RENDER-26a) ends notes the preview dropped and reschedules moved ends.
- **Outside writes during the lease:** a settle that swaps the accepted lane data in would overwrite the
  preview; the lease must re-send its preview on top (other clips of the lane changed) or end (its own
  clip changed: the commit would conflict anyway).
- **A new renderer or plan while the lease lives** (a code edit elsewhere): the lease's lane data goes
  into it before it's sent.
- **Commit:** one `Clips` Tx on the actor; when it's accepted, the engine already plays the same data.
  **Cancel:** swap the accepted data back.
- **Audio lanes:** a preview needs renders, so no cheap `SwapLane` (design-7 §8.2 keeps audio swaps
  inside `Activate`). Dragging an audio clip previews on release.
- **The data plane's C20:** the step 6 design's option (2) for PLANE-5's missed 20 ms is "the `SwapLane`
  sent as a preview once the candidate passes … as WRITE-22a's lane lease previews". A lane lease's
  preview path is the same machinery.

---

## 7. Audition

**Contract.** MON-7: "Audition is not an overlay. It is a `Live(Event)` source merged into a lane's event
buffer." RENDER-14: one `Event` shape for every source; "live input and audition merge into a lane's
buffer (later)". RENDER-6a: "Audition and live input work while stopped"; the engine may idle once
silent for 8 blocks, and any message wakes it. RENDER-28: `Live(Event)` is in the engine's message list;
structural messages go over a bounded queue and are never dropped. The deferred table: MIDI and
computer-keyboard input come "after audition (step 8)", on the same path. Planes table: audition is the
control plane.

**At HEAD:**

- `core/runtime/src/event.rs`: `Event::{On, Off, NoteExpression, Param, Midi}`, the whole shape; only
  `On` and `Off` are produced. `Events` has a fixed capacity; a full buffer refuses an `On` and counts
  it; `Off`s always have room because a scheduler sizes the buffer for one per note it can hold.
- `core/runtime/src/sched.rs`: each notes lane's scheduler fills its buffer from the transport and owns
  its sounding notes.
- `core/runtime/src/engine.rs`: `Msg::{Activate, SwapLane, Transport, Panic}`; no `Live`.
- `Player::panic()` exists (every note off, every instance reset, RENDER-10), but there is no route and
  no button (gap #8 asks for "an all-notes-off button (panic)").
- No audition route, and the piano roll makes no sound.

**What to build:**

- `Msg::Live { generation, slot, event }` (or a dedicated lock-free ring, since MIDI input will want
  sample-stamped events at a higher rate), merged into the lane's buffer at the next block line.
- **Room:** each lane's buffer reserves space for live notes, or a live `On` can be refused when the
  song is busy.
- **Ids:** live notes need ids that never collide with scheduled ones in the same consumer (a plugin
  matches Offs by id). Step 7 makes note ids engine-wide (`Renderer.next_note`, design-7 §8.1); live
  notes take ids from the same counter, which the control plane can't reach, so the engine assigns them
  and the control plane names a live note by its own handle.
- **Ending live notes:** on the user's release; after a fixed time for a click in the piano roll (a note
  drawn is auditioned for its length, capped); on panic; when an activation replaces the lane's
  consumer (RENDER-26a's first rule applies to live notes too). Whether stop, seek and a loop wrap end
  them is a decision: a held MIDI key in other DAWs keeps sounding through a stop.
- **Routing:** `POST /api/graph/audition {module, lane, pitch, velocity, on|off}` (or a WebSocket message,
  for latency), resolved on the control plane, never the session. A lane nothing reads is silent, and
  the piano roll could say so; a monitoring-muted track is silent too, which is what Ableton does.
- **The piano roll:** audition on draw, on a pitch change while dragging, and on its keyboard strip
  (hold to sound).

---

## 8. Loop and transport

Built (step 3a and after; `crates/server/src/graph.rs`, `core/runtime/src/transport.rs`,
`web/src/components/GraphCanvas.tsx` and `SongTimeline.tsx`):

- play, stop, toggle, restart (`POST /api/graph/transport`), seek (`/seek`, a jump at the next block),
  the loop (`/loop {start?, end?, on?}`: a range in beats, kept while off, at least a minimum length,
  refused by the engine when a tempo change makes it too short);
- in the UI: Space, Enter or Home, the loop button (with no range, it loops the 4 bars from the
  playhead), drag across the ruler to set a range, click the ruler to seek (`ux8/shots/b04…`, `b06…`);
- status polled every 200 ms (`GraphCanvas`), position as bar.beat and minutes:seconds.

Not built: the loop range isn't kept in `.music2/` (the HOME table puts it there) and is lost on close;
no ⌘L (gap #7 names it); no metronome or count-in (#19); no panic (§7); RENDER-10's "reset on stop" user
choice has no setting; no follow-playhead scrolling; no "return to where play started" on stop.

gaps.md is stale here: #5 (fast seeking and edits) was built in steps 3 and 6, and #7 (loop) is built
apart from ⌘L and keeping the range. Its "What works today" doesn't mention the loop at all, while the
README does.

---

## 9. What the contract must say before 8b is built

| Rule | Gap | Suggested change |
|---|---|---|
| WRITE-10 | No rows for delete, duplicate, rename key, tempo/meter, clips by id, layout writes | Add them (§1.2's table) |
| WRITE-12 | The notes write's exception ("the piano roll sends no lane hash") | Closed by clip editing by id: the client sends its base |
| WRITE-8 / MON-5 | Says `muted` sets a cell with no rebuild; the delta counts every module change as structural | `structural()` excludes `Muted`; `reach` maps it to the module's overlay cells |
| ID-11 | `Module { key, label, muted }`: `muted` not built | Lands with the SDK option |
| ID-17 / design-7 §3.4 | A key rename keeps data, but step 7 never matches instances across module keys | Decide: add the `Rekeyed` pass in 8b, or accept a restart of the renamed track's instruments |
| ID-18 | "Following the graph edge" through intermediate calls is unspecified | Say: downstream from the export until a channel's `in`; none or two means no channel |
| PKG-5 | The mixer's `db` optional means some channels have no fader | Decide between `db` not optional (if bit-exact) and first-touch structural writes; the insertion planner learns dictionary entries either way |
| MON-1 | Where the overlay gain lives (a built-in op in `Flat`, or added by the runtime), and that it's in every plan, offline included, at exactly 1.0 when inactive | State both, and that its presence never changes a pure render's bits |
| MON-3 | "On a path through a soloed module": up and downstream, over non-sidechain audio edges between modules | Say so, with the bus example |
| MON-4 | A read-only window's overlays | In memory only |
| RENDER-22a | A one-block ramp is ~2.7 ms; low notes can click within the gate's limit | Consider a longer ramp for overlay cells (several blocks), which the gate allows |
| RENDER-27a | "At the next block" vs. an `Activate` per update | "At the first block after its plan is prepared", and the control plane keeps one in flight |
| HOME table, HOME-1 | No home for the arrangement; none for a song key or scale | Add `arrangement` (or say tempo is `lane`-like data); decide key/scale |
| FMT-13 | Key and scale; tempo ramps | Owner decisions; each is a format addition if it goes in the file |
| FMT-19 | Nothing reads, watches or bases `layout.json` | The server reads and watches it; layout builders base it from disk |
| Gate 8 | "A compressor keyed from the kick's audio": std has none | std adds an optional audio key input (`port.audio({ sidechain: true })`) |
| Deferred table | Meters' trigger is "Mixer meters" | If 8b's mixer shows meters, render taps come into 8b |
| MON-7 / RENDER-14 | What stop, seek, wrap and a replaced consumer do to a live note; who assigns its id | State it with audition |

---

## 10. gaps.md's top ten against 8b

| # | Gap | 8b covers | Left over |
|---|---|---|---|
| 1 | Add a track without writing code | **Add track**, duplicate | Swapping a track's instrument; plugin instruments from the picker (#32) unless the picker lists them |
| 2 | More instruments | No (std work) | Add track needs std's instruments to declare `editor` metadata |
| 3 | A drum kit | No | Drum names in the piano roll |
| 4 | A mixer panel | **Mixer, mute and solo, mute in song** | Master fader (std), meters (#20, render taps), sends and buses (#27), sidechain picker (#21) |
| 5 | Fast seeking and edits | Built (steps 3, 6); the arena (7) and partitions (8a) finish it | gaps.md entry is stale |
| 6 | Tempo, time signature, key, title in the top bar | **Tempo bar with its lease**, meter | Key and scale (no home), title (manifest edit) |
| 7 | Loop | Built | ⌘L, keeping the range in `.music2/`; gaps.md entry is stale |
| 8 | Hear notes in the piano roll, panic | **Audition** | Panic needs only a route and a button |
| 9 | Clip editing on the timeline | **Clip editing by id**, lane leases | Audio clips (#18) need import first |
| 10 | Rename, delete, reorder, recolour tracks | **Rename (key), delete** | Reorder and recolour: `layout.json` (§2.6), not in 8b's list |

Further down the list, 8b also reaches #14 (draw automation: clips by id on control lanes, lane leases),
#15 (note tools: the clip builder is their base), #17 (zoom and right-click menus: every 8b gesture
needs a menu entry), #29 (readable undo messages: every new builder's label pair), #37 (`m2 add-track`,
`m2 tempo`: each is small once its builder exists) and #38 (the API: each builder is a route).

---

## 11. What a pro producer expects that neither the contract nor gaps.md mentions

Checked by searching both documents. The ones marked **8b** fall inside 8b's own features and are cheap
to do with them; the rest are for later.

| Feature | Why a producer misses it | Nearest thing today | When |
|---|---|---|---|
| **Snap and grid on the timeline**, with a choice (bar, beat, 1/8 … off, triplets) and a grid that follows zoom | Clips move in whole beats only, with no snap control; the piano roll has its own snap | `NoteView`'s snap menu | **8b** (clip editing) |
| **Nudging clips** with the arrow keys, by the grid | Fine placement without the mouse | Notes nudge in the piano roll | **8b** |
| **A range (time) selection** across tracks, marquee selection of clips, cut/copy/paste of a range | Arranging by sections; #23 covers named sections, not ad hoc ranges | One picked clip | **8b** (clip editing), then #23 |
| **Locators and markers** apart from sections: drop a marker, jump between markers | Navigation in a long song | Ruler marks for tempo and meter changes | later (with #23) |
| **Solo modes**: exclusive solo (⌥-click), "clear all solos", a visible "something is soloed or muted" indicator | Overlays persist in `.music2/`, so a song can reopen half silent | none | **8b** (mute and solo) |
| **Mute and solo from the track header**, and M/S keyboard shortcuts on the selected track, drag across buttons to toggle many | Every DAW puts them on the track header too | none | **8b** |
| **Mute fades long enough for bass** | A 2.7 ms ramp can click on a low note (§3.5) | RENDER-22a's one block | **8b** |
| **Stable track colours, chosen per track** | Colours shift when a track is added | index palette | **8b** (add track writes one) |
| **Folder tracks or track stacks, VCA faders** | Grouping 20 drum tracks; #27 has "group buses" only | none | later |
| **Pre/post-fader sends and return tracks** | #27 covers sends and buses, not their tap point | none | later |
| **Channel trim, polarity, mono, pan law** | Standard strip controls | `gain`, `pan`, `widen` as effects | later |
| **A CPU (DSP load) meter and a dropout counter** in the top bar | Knowing when the song is too heavy; 8a's gate already measures the callback's share of its budget | none in the UI | with 8a |
| **Tempo ramps** (accelerando, ritardando) and **tap tempo** | `TempoMap` is piecewise constant | tempo entries | later (a format addition) |
| **A tempo lane** on the timeline to draw tempo changes | Editing more than one tempo | ruler marks | **8b** (the tempo bar's other half) |
| **Follow playhead** (auto-scroll) and **return to start position on stop** | Long songs at a fixed 24 px per beat | none | with #17 |
| **Time display modes**: bars/beats, minutes:seconds, samples, SMPTE | Video work and precise edits | bar.beat and seconds | later |
| **Punch in/out** and **count-in** for recording | #19 has count-in "with recording"; punch is absent | none | with #24/#25 |
| **MIDI learn** (a controller knob to an app knob) and **control surfaces** | Hands-on mixing with a controller | none | after MIDI input |
| **An undo history list** (jump back several steps) | #29 covers readable messages, not a list | 500 steps in memory | later (labels exist) |
| **Preferences**: default snap, "reset on stop" (RENDER-10's user choice), auto-scroll, metronome sound, MIDI device, plugin folders, new-project defaults | #31 covers the audio device, buffer and folder only | env vars (`CLAP_PATH`, `M2_*`) | later |
| **Keyboard shortcut list and customisation** | Speed | a fixed table in the README | later |
| **Track height, collapsing lanes, hiding tracks** | Big sessions | fixed heights per lane kind | with #17 |
| **Ghost notes** of other clips in the piano roll, **MIDI CC lanes** under it | Writing parts against each other | none | later |
| **A step sequencer or drum grid** | Beat programming (with #3's drum names) | the piano roll | later |
| **Plugin bypass and plugin delay display** | #21 has effect bypass; latency comp is deferred | none | later |

---

## 12. A build order that fits

Each slice is green on its own, and each one that touches core changes the contract first.

1. **Clip editing by id** (builder + timeline UI, no engine change): clip-level requests for notes and
   control lanes, cross-lane moves, duplicate, copy/paste, resize, split; closes WRITE-12's exception
   and gives #14 its writer. Timeline snap and nudge with it. Coordinate with the G11 work on
   `notes.rs`.
2. **Audition and panic** (engine `Live`, route, piano roll). Small, and the piano roll is useless
   without it. Ids from step 7's engine-wide counter.
3. **Lane leases** (control plane, on `PlanInfo`): notes drags and automation drawing heard while
   dragging.
4. **Track builders**: add (with std instrument metadata, binding names, colour in `layout.json`, the
   reader and watch of `layout.json`), delete (with references found), duplicate, key rename (with
   Restore, and the step 7 `Rekeyed` pass or a stated restart). `m2 add-track` and friends.
5. **Overlays and mute in song** (core: IR, SDK, delta, runtime op; control plane; `.music2/overlays.json`;
   export warning), with the gate's fixtures (a keyed compressor in std, `follow(kick.out)`, a
   track-wrapped sine).
6. **The mixer panel** on the view's knobs, ID-18's channel lookup, dictionary-entry insertion (and the
   `db` decision), a master fader in std, mute/solo/mute-in-song on each strip and on track headers.
   Meters only if render taps come in.
7. **The tempo bar and its lease** (builder first, then the lease on step 7's identity activation), the
   meter field, and the `Home` decision.

Steps 1–4 and the tempo builder can start before 8a; 3, 5's engine part and the tempo lease are best
after step 7, since they'd otherwise be written once against `Plays` and again against `PlanInfo`.

---

## 13. Pointers (HEAD `cbf5eb9`)

| What | Where |
|---|---|
| Track, lane, song SDK | `core/script/sdk/core.ts`: `track`, `TrackOptions`, `options`, `declareLane`, `ctxFor`, `__build` |
| IR module, frames, origins | `core/ir/src/lib.rs`: `Module`, `Frame`, `Origin`, `Slot` |
| Flat graph, sidechains | `core/ir/src/check.rs`: `Flat`, `FNode`, `FOp`, `sidechains` |
| Tx types | `core/song/src/tx.rs`: `Op`, `Expect`, `Confined`, `ModuleChange` |
| Delta | `core/session/src/tx/delta.rs`: `GraphDelta`, `structural`, `modules` |
| Expectations | `core/session/src/tx/expect.rs`: `confined` |
| Arrangement | `core/song/src/arrangement.rs`: `Arrangement`, `TempoMap`, `canonical` |
| Lane format, ids | `core/song/src/lane.rs`, `core/song/src/key.rs` (`ClipId`) |
| Homes | `core/session/src/lib.rs`: `Home` |
| Builders | `crates/editor/src/{plan,rename,automate,notes,data,keys,sites,lens,controls,view,timeline}.rs` |
| Engine messages | `core/runtime/src/engine.rs`: `Msg`; `core/play/src/lib.rs`: `Player::{swap_lane, set_cells, panic, seek, set_loop}` |
| Events, schedulers, transport | `core/runtime/src/{event,sched,transport}.rs` |
| Plugin transport | `core/plugins/src/audio.rs`: `transport_event` |
| How changes reach the engine | `crates/server/src/graph/actor.rs`: `Way`, `reach`, `way`, `adopt`, `same_layout` |
| Leases, control plane | `crates/server/src/graph/lease.rs`; `crates/server/src/graph.rs`: `Timing`, `Status`, `seek`, `set_loop`, `transport` |
| Routes | `crates/server/src/routes.rs`: `router`, `WRITES` |
| std mixer, duck, follow, compress, gain | `std/src/mix.ts`, `std/src/fx.ts`, `std/src/dsp.ts`, `std/kernels-src/src/lib.rs` (`Gain`) |
| Timeline, piano roll, top bar | `web/src/components/{SongTimeline,NoteView,GraphCanvas,TrackName}.tsx`, `web/src/lib/graph.ts` |
| Watcher | `core/session/src/watch.rs`, `crates/server/src/graph/watcher.rs` |
| Reference projects, gate songs | `core/session/src/reference.rs`, `crates/gates/src/songs.rs` |
| Step 7's engine | `.git/m2-step7/design-7.md` §1, §3.4, §5.3, §7.5, §8.1–8.4, §14 |
| Step 6's leftovers for 8 | `.git/m2-step6-resume/design-6.md` §25, C20 |
