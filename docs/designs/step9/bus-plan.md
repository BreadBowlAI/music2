# Track bus: sends and buses

gaps.md #27. Branch `bus`, from `main` at `e7518f4`.

## In short

- A **bus** is a track that plays no notes or clips. It plays what other tracks send it: a shared reverb, a delay,
  or a group (all the drums through one compressor and one fader).
- **All routing lives in one place: the mixer in `song.ts`.** A channel gets `sends` (knobs, after the fader or
  before it) and `to` (go to a group bus instead of the master). A bus's own file holds only its effects.
- Three slices, in this order. Each ends with a commit, and its tests are shown red before they pass.

  | # | Slice | What you get |
  |---|---|---|
  | 1 | Buses in song code | `bus()` in `@m2/core`; sends and groups in std's mixer; the engine plays them on any thread count, in time, with mute and solo right; a bus that feeds itself is refused in plain words |
  | 2 | The edits | Add a bus, add or remove a send, turn a send, send before or after the fader, route a track to a group. Each is one undo step |
  | 3 | The Mix view | Bus strips, a send knob on each strip, an output picker, an "Add bus" menu |

- **No sound changes.** A song with no buses makes the same graph and the same bits. `e2e/renders.sha256` stays.
- **New API, so new versions:** `@m2/core` 1.5.0 and std 0.4.0 (FMT-3: new API a song must not meet in an old std).
- **Meters after the fader** (gap #20) don't fall out: they need taps in the engine. Logged; bus strips get the
  same meter tracks have (before the fader).
- One `ci/check.sh --gates` at the end of the track. Linux timings are information only (PLAT-1).

## Words used here

| Word | Meaning |
|---|---|
| bus | a track with no notes or clips; its sound comes from other tracks |
| send | a copy of a track's sound going to a bus, at its own level (a knob) |
| after the fader (post) | the send follows the track's fader: turn the track down, its reverb goes down too |
| before the fader (pre) | the send ignores the fader |
| group | a bus that tracks go *to* instead of the master (all the drums into one "Drums" bus) |
| feed | how the song hands a bus its sound, in code |
| partition | a part of the graph run as one task on one thread (RENDER-2) |

## How it is written

A bus file (`tracks/verb.ts`). `input` is what the song feeds it:

```ts
import { bus } from "@m2/core";
import { reverb } from "std/fx";

export default bus("verb", { name: "Reverb" }, ({ input }) => reverb(input, { mix: 1 }));
```

`song.ts`. Sends and groups are written on the channels; buses are channels too:

```ts
master(
  mixer({
    kick: { in: kick.out, to: "drums" },                       // into the Drums group
    snare: { in: snare.out, db: db(-3), to: "drums" },
    drums: drums.out,                                          // the group's own strip
    vox: { in: vox.out, db: db(-3), sends: { verb: db(-12) } }, // after the fader
    keys: { in: keys.out, sends: { verb: { db: db(-18), pre: true } } }, // before the fader
    verb: verb.out,                                            // the reverb's strip
  }),
)
```

What std's mixer does with it, inside the one `mixer` call:

| Channel has | The mixer builds |
|---|---|
| nothing new | today's strip and sum, node for node (the same graph) |
| `sends: { b: level }` | the strip (after pan and fader) times `level`, added to bus `b`'s feed |
| `pre: true` on a send | the channel's input (before pan and fader) times `level` |
| `to: "b"` | the strip goes into bus `b`'s feed, not the master sum |
| `in` reading a bus (`verb.out`) | a feed for that bus: the sum of what is sent to it, or silence. `feed(verb, sum)` (core) |

In the graph: the bus module imports the song's export `bus:verb` (or the nearest name the key rule allows). The
song reads `verb.out` back. So `song` and `verb` read each other both ways. RENDER-2 already splits such modules by
crossings: the strips run with their tracks, then the feed sums (song), then the buses (each its own partition, in
parallel), then the master sum (song). No engine change is expected; slice 1's first test proves it.

## Slice 1: buses in song code

**Goal.** A hand-written song with a reverb bus, a delay bus and a drum group builds, plays and exports. Same bits
on 1 thread or many. A latent plugin on a bus stays in time. Mute and solo follow the routing. A loop is refused in
plain words.

**Crates and files.**

| Where | What changes |
|---|---|
| `core/script/sdk/core.ts` | `bus(key, opts, fn)`: a track whose `fn` gets `input`. `feed(bus, x)`: song code only, once per bus. A bus declaring notes or audio lanes is refused (control lanes, for automation, are fine). PKG-6: an `each` entry may hold a nested `each` and settings; a bare value fills an entry with no input and one control. A new setting kind `key` (a string that follows the key rule) |
| `core/script/src/lib.rs`, `component.rs` | `SDK_VERSION` 1.5.0. `Descriptor::control` follows `channels.vox.sends.verb.db` (five parts) |
| `core/ir/src/lib.rs` | `Module.bus`, written only when true (ID-11), like `muted` |
| `core/ir/src/check.rs` | `routes`: for each bus fed by the song, the tracks whose sites into the song reach its feed over audio edges (sidechains aside). A loop through code stays the cycle error; its message names the modules on the loop |
| `std/src/mix.ts` | `sends`, `pre`, `to` on a channel; feeds every bus channel; refuses a loop among its channels: "verb feeds itself: verb → delay → verb. A bus can't feed itself. Take away one of these sends or outputs." (keys only: song code can't see names, ID-10) |
| `std/m2pkg.toml`, every shipped `music2.toml` | std 0.4.0; shipped songs ask for "0.4" (as track pm did) |
| `crates/server/src/graph/overlay.rs` | `values` adds the routes as edges (below) |
| fixture `core/session/tests/fixtures/buses/` | a reverb bus, a delay bus with a latent probe, a drum group |
| `docs/architecture.md`, `docs/song-as-program.md` | the rules below |

**Mute and solo (MON-3, amended).** A bus is a track, so it has cells where the song reads it. The feed into a bus
is read from the song, which is not a track, so it is not a site. Instead the checker's routes `m → b` join the
"who feeds whom" graph:

| You do | You hear |
|---|---|
| Mute a track | nothing from it: dry, sends and group, since all start after its one cell |
| Solo a track | it, and every bus it feeds (through sends or `to`), and the buses those feed |
| Solo a bus | it, and every track that feeds it. Their dry sound too: one cell serves both ways (logged) |
| Mute a group | its tracks go silent with it |

Hand-written buses that read tracks directly (`solo_keeps_a_bus_fed_and_its_sources`) keep today's rule.

**Tests (red first).**

| Test | Where | Red because |
|---|---|---|
| `a_bus_reads_what_the_song_feeds_it` (IR: `bus: true`, an import of the song's feed) | `core/script/src/bus_tests.rs` | no `bus()` |
| `a_bus_with_notes_is_refused`, `feeding_a_track_that_is_not_a_bus_is_refused`, `a_bus_fed_twice_is_refused`, `an_unfed_bus_is_refused` | same | no checks |
| `a_nested_dictionary_control_is_found` | `core/script/src/tests.rs` | three parts only |
| `a_send_after_the_fader_follows_it`, `a_send_before_the_fader_ignores_it`, `a_track_sent_to_a_group_leaves_the_master` (renders: the bus's input against the strip, sample for sample) | `core/session/src/gate_tests.rs` | no `sends`, `to` |
| `a_bus_that_feeds_itself_is_refused_in_plain_words` (std's loop, and one through code) | same | no message |
| `a_song_and_its_buses_split_into_partitions_with_no_cycle` | `core/runtime/src/tests/` | proves RENDER-2 as written |
| `buses_give_the_same_bits_on_any_thread_count` | `core/play/tests/bit_identity.rs` | no fixture |
| `a_latent_plugin_on_a_bus_stays_aligned` (the probe on a bus; the dry path, inverted, sums to exactly 0) | `core/play/tests/` | no fixture |
| `solo_keeps_the_buses_a_track_feeds`, `soloing_a_bus_keeps_what_feeds_it`, `muting_a_track_silences_its_sends` | `crates/server/src/graph/overlay_tests.rs` | no routes |

**Done when:** the tests pass; every existing render, IR hash and `e2e/renders.sha256` line is unchanged
(`fixture_renders_change_only_with_stds_version` re-recorded under 0.4.0 with the same hashes); the mac lint is
clean; the contract's new rules are in, each with its *Today* line.

## Slice 2: the edits

**Goal.** Every bus edit the Mix view needs, as Tx builders in `crates/editor`, each one undo step, each refused
before writing when it would make a loop. Routes for them in `crates/server`.

| Builder (`crates/editor/src/bus.rs`) | What it writes |
|---|---|
| `add_bus(label, kind, after)` | `tracks/<key>.ts` with `bus(...)` (kind: Reverb = std reverb at mix 1; Delay = std delay at mix 1; Group = `input` alone), its import in `song.ts`, its channel `<key>: <key>.out`, its layout place. Reuses `tracks::add_tracks`' writing |
| `add_send(channel, bus, pre)` | `sends: { bus: db(-12) }` on the channel (an entry, or the object), `pre: true` when asked |
| `remove_send(channel, bus)` | the entry; the object too when it was the last |
| `set_pre(channel, bus, pre)` | the send's `pre` |
| send level | the fader's knob write (a Confined Tx, a cell write, no rebuild), extended to nested ids. A new builder only if that path can't name them |
| `route(channel, to)` | `to: "<bus>"`, or none for the master |
| `tracks::delete` of a bus | also removes the sends to it, and routes its tracks back to the master (its `Dependent` list says so first) |

- Finding a track's call (`rename::track_call` and its users: rename, mute in song, delete, duplicate) accepts
  `bus` as well as `track`.
- `mix::view` gains, per strip: `bus: bool`, `sends: [{bus, knob, pre, lane}]`, `output` (none = the master),
  and the list of buses a strip may send or go to without a loop. Bus strips come after the tracks, before the master.
- Server: `POST /api/graph/mix/bus`, `/mix/send` (add, remove, pre), `/mix/route`; the knob route already writes
  levels. The CLI gets nothing new (its missing commands are #37).

**Tests (red first),** in `crates/editor/src/bus_tests.rs` unless named:

| Test | Shows |
|---|---|
| `add_bus_writes_a_bus_file_and_a_channel` | the files, the graph's new bus module, one undo step back to the same bytes |
| `add_send_and_remove_it`, `a_send_can_go_before_the_fader` | the code, one undo step each |
| `a_send_level_is_a_cell_write` | no new plan, like a fader |
| `route_takes_a_track_to_a_group_and_back` | the code, and the master sum without it |
| `a_send_or_route_that_makes_a_loop_is_refused_before_writing` | "Delay already feeds Reverb, so Reverb can't send to Delay" (names, not keys), nothing written |
| `deleting_a_bus_takes_its_sends_and_routes_with_it` | the song still builds; its tracks go to the master |
| `a_bus_is_renamed_and_muted_like_a_track` | `track_call` finds `bus` |
| `the_mix_view_lists_buses_sends_and_outputs` | `mix::view` |
| `the_bus_routes_answer_with_the_new_view` | `crates/server/src/graph/tests.rs` |

**Done when:** the tests pass; a new send or route plays within one activation (RENDER-25a fades, no click);
`cargo test -p m2-editor -p m2-server` green.

## Slice 3: the Mix view

**Goal.** A beginner adds a reverb and hears it, without code.

- `web/src/components/Mix.tsx`: an "Add bus" menu (Reverb, Delay, Group). Bus strips, marked as buses, with their
  effect and fader. On each track strip, one small knob per bus (its send level; a dot when it is before the
  fader, set in its menu), and a "+" to add a send. An output picker under the fader: Master, or a group bus.
  Buses that would make a loop are greyed, with why.
- Meters: a bus strip's meter is its module's, before its fader, as for tracks (`TrackMeter`).
- The timeline shows a bus as a track row with no lanes to fill and no record button.
- Docs: `README.md` (what the app does), gaps.md #27 out and the new lines in, `docs/mac-run.md` by-ear steps,
  HANDOFF.

**Tests (red first).** `e2e/bus-mix.mjs`: add a Reverb bus; a send knob shows on every strip; turning it is heard
(the bus's meter moves); send before the fader; route a track to a Group and its strip's output says so; each
undone in one step; a loop choice is greyed. Red on today's UI (no "Add bus"). A look at it with `app-shot.sh` on
the Xvfb screen :98.

**Done when:** the suite passes with the other browser suites; `ci/check.sh --gates` passes once.

## Decisions

| # | Decision | Why |
|---|---|---|
| 1 | A bus is a track: `bus()` in `@m2/core`, `Module.bus` in the IR | Keys, files, rename, delete, layout, mute in song, mute and solo cells and its own partition all come free |
| 2 | Routing lives in the song's one mixer, not in bus files | A send after the fader needs the fader, which is in the mixer. One place holds every fader and send, as the Mix view shows them |
| 3 | The song feeds a bus (`feed`); the bus reads `input` | The graph still only pulls: the bus imports the song's export |
| 4 | Sends and `to` name the bus by its channel key in the same mixer | Keys never change in the app (rename is labels only), so routes never break |
| 5 | A new send: after the fader, −12 dB (range −60 to +6) | You hear it at once, under the dry sound; most DAWs default to after the fader |
| 6 | After the fader means after pan and fader; before means before both | One rule, said once |
| 7 | A channel goes to one place: the master, or one bus. A bus may go to another bus | Groups of groups work; loops are refused |
| 8 | Loops refused three ways: std's message, the checker's cycle error naming the loop, and the editor before writing | Plain words at every door |
| 9 | A bus nobody feeds is a build error | The mixer always feeds its buses; only hand-written code meets it |
| 10 | A bus may have control lanes, not notes or audio lanes | Automate the reverb's mix; nothing to play or record |
| 11 | Soloing a bus keeps its sources' dry sound too | One cell per track into the song; a wet-only solo needs a cell per path (logged) |
| 12 | No engine change; the tests prove partitions, bits and latency | RENDER-2 and RENDER-17 to 21 already cover modules that read each other |
| 13 | `@m2/core` 1.5.0, std 0.4.0, shipped songs ask for "0.4" | FMT-3; no render changes |
| 14 | No new external crate | None needed |

## Contract changes

| Rule | Change |
|---|---|
| RENDER-34 (new) | Buses: a bus is a track fed by the song (`feed`), read as `input`; fed once; no notes or audio lanes. Song and buses split by crossings (RENDER-2). A loop is a build error naming it |
| MON-3 | Routes join the "who feeds whom" graph; the table above |
| PKG-6 | Nested dictionaries and settings in an entry; the `key` setting; the bare one-control entry |
| song-as-program.md | "References between modules": buses, sends and groups, with the example above |

## Risks

| Risk | What we do |
|---|---|
| Nested ids (`channels.vox.sends.verb.db`) break code that splits ids in three | Slice 1 tests the lookup; slice 2 the knob write and automation on a send |
| The song reading a bus that reads the song trips an assumption that `song` is read by nobody | Slice 1's first test is the partition and plan test |
| Solo values wrong with routes | A table test of every row above |
| A bus key clashes with an import (`reverb` the bus, `reverb` from std) | `add_bus` picks a free binding, as `add_tracks` does |
| Track-only features offered on a bus (record, piano roll, instrument card) | Slice 3 hides them; any missed goes to gaps.md |

## To gaps.md instead (one line each)

- Meters after the fader, buses included (#20 stays).
- Soloing a bus plays its sources dry too; a wet-only solo needs a cell per path.
- A send's own pan, and sends in mono.
- Solo-safe buses and "solo in place" options.
- Folding a group's tracks under it in the timeline.
- Sending from inside a track's chain (before its effects); today a send starts at the track's output.
- Bus commands in the CLI (part of #37).
