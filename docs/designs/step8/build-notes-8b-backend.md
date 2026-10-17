# 8b back ends: build notes

The `8b-backend` track builds the back ends of 8b.4 (mute, solo, mute in song), 8b.5 (clips), 8b.6 (audition) and
8b.8 (meters): engine, IR, session, Tx builders, routes, CLI and contract. No UI. The plan is
`~/m2-runs/s8b/refresh.md` (its Attack and Decisions win) on top of `design-8b.md`.

## In short

| Slice | What | State |
|---|---|---|
| B1 | The IR's `muted`, and where overlays go (`Flat.overlays`, `Flat.muted`) | built |
| B2 | The overlay op in every plan; overlay cells; the `muted` track option; the delta's `Muted` | built |
| B3 | Server mute and solo: the path rule, `.music2/overlays.json`, `POST /api/graph/overlay`, new plans' values, mute in song as cell writes, renames, the export's warning | built |

---

## B1. Where overlays go

**In short:** the checker now says where mute and solo act. Nothing plays it yet.

| Piece | What it does | Where |
|---|---|---|
| `Module.muted` | Mute in song, in the IR. Written only when true, so old IRs keep their bytes and hash. | `core/ir/src/lib.rs` |
| `Flat.muted` | The muted tracks, by key, sorted. Refused on `song`, and on a module that isn't a track. | `core/ir/src/check.rs` |
| `Flat.overlays` | Each place a track's audio enters another module: an input, a voice's capture, a feedback, or the song's output. | `core/ir/src/check.rs` |

**What isn't a site:**
- a control export, such as `kick.env`;
- a sidechain input: an input made inside a frame that recorded a sidechain, which reads the signal that frame got;
- a plugin's audio inputs other than `in` (PKG-13);
- a voice's capture that a sidechain input reads.

**Choices made here:**
- A site also carries `into` (the module that reads it) and `src` (what it reads). The server's solo rule needs
  `into`; the runtime needs `src`. Both come from the graph, so the list stays "same code, same list".
- `Consumer` has four cases, as the runtime's edges do: `Input`, `Capture`, `Feedback`, `Out` (A5).
- `Flat.side_inputs` isn't a field (S3). The checker works out sidechain inputs inside `check()`.
- No SDK option yet (G2): `muted: true` would be legal and do nothing until the op is built.

**Tests:** `core/ir/src/overlay_tests.rs` (new file, G4): the IR hash, the song's output, every input, the
sidechain input (with and without its record), a control export, a capture and a feedback, a track read by a
track, and `muted` refused on `song` and on a non-track. Written with the code, not before it.

**Checks:** `m2-ir`, `m2-script`, `m2-session` tests.

**Contract:** ID-11 *Today*, PKG-13 *Today*.

---

## B2. The overlay op

**In short:** every plan now has a gain node at each site. Mute in song silences a track in every render. A mute
glides over 10 ms. An edit while a track is muted never lets it through.

| Piece | What it does | Where |
|---|---|---|
| `Exec::Overlay` | `x × gain`. At 1 it copies (no bit changes). At 0 it writes +0.0 (R1). Otherwise the gain moves 1/480 a frame at 48 kHz. | `core/runtime/src/overlay.rs` |
| `overlay::place` | Puts one node per site into every plan, offline too. Wires the consumer, or the fade on that input, to it. | `core/runtime/src/overlay.rs` |
| Window overlays | A fade's old side, and a ghost's input, that read an overlay before read one for the window (A3). No cell; the new plan's value for the same track and consumer. | `core/runtime/src/overlay.rs` |
| Order | Each overlay runs right after its source (A4). Window ones leave with the window. | `core/runtime/src/overlay.rs` |
| Overlay cells | Their own table in `Cells`, ids after every value cell (A2). A write sets the node's target; the node glides. | `core/runtime/src/cells.rs` |
| `PlanInfo.overlays` | Per site: the track, the module that reads it, the cell. | `core/runtime/src/plan.rs` |
| `Renderer::set_overlays` | A plan not yet sent starts at these values: no glide at entry. Writes no slot (A7). | `core/runtime/src/lib.rs` |
| `track(k, { muted: true }, fn)` | The option, in `@m2/core` 1.2.0 (G2, Q16). Emitted only when true. | `core/script/sdk/core.ts` |
| `ModuleChange::Muted` | The delta reports a `muted` change, also on a rekeyed module. `structural()` is unchanged (A1). | `core/session/src/tx/delta.rs` |

**Tests** (new files, G4):

| Test | Where | Red first? |
|---|---|---|
| `an_overlay_at_unity_changes_no_bit` (against the graph with no overlays) | `core/runtime/src/tests/overlay.rs` | — |
| `an_overlay_glides_over_ten_milliseconds` (each sample, to the bit) | same | — |
| `muted_in_song_silences_an_offline_render` (and a second track plays on, to the bit) | same | — |
| `a_new_plan_starts_at_its_overlay_values` (and a panic leaves them; the control without them) | same | — |
| `an_edit_while_muted_keeps_the_track_silent` (a replaced consumer, a faded input; the unmuted control) | same | red with the window overlays turned off: peak 0.5 leaked |
| `muted_is_a_track_option`, `an_unknown_option_is_still_refused` | `core/script/src/mute_tests.rs` | — |
| `a_mute_in_song_toggle_is_a_muted_change` | `core/session/src/tx/muted_tests.rs` | red before the delta change |
| `toggling_mute_makes_no_step_larger_than_twice_the_sines` (440 and 50 Hz; buffers 128, 37, 512) | `core/play/tests/mute.rs` | red with an instant step (a step of 0.9997) |
| `a_monitoring_mute_live_equals_mute_in_song_offline` (`f32` equality, A9) | same | — |
| `overlay_writes_never_allocate` (writes while playing, and an activation) | `core/play/tests/no_alloc.rs` | — |

**What we learned:**
- Gate 8's click bound doesn't tell a 10 ms glide from a one-block ramp. The design adds the two worst cases
  (0.0065 + 0.0078 > 0.0131 at 50 Hz). But the sine's slope is largest where the sine is near 0, and the gain's
  effect is largest where the sine is near 1. So a one-block ramp passes too (tried: green). Only a step fails.
  The runtime test pins the 10 ms glide instead. RENDER-22a says so.
- Until B3, a mute-in-song toggle is still an activation. The new plan starts at its new value, so the toggle is a
  step for now. B3 makes it a cell write, which glides.
- `m2-play`'s click test builds its tone song in the test itself (G3). Nothing in `crates/gates` changed.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-ir`, `m2-runtime`, `m2-play`, `m2-script`,
`m2-session`; their tests; `e2e/baseline.sh --tree` against `e2e/renders.sha256`; `renders.json` unchanged.

**Contract:** MON-1 (sites, the op, the window rule) and its *Today*, MON-5 *Today*, RENDER-22a (the glide, R3, the
50 Hz arithmetic), RENDER-23 *Today*.

---

## B3. Mute and solo on the server

**In short:** `POST /api/graph/overlay` mutes and solos while you listen. It never waits for the session. The
choice is saved in `.music2/overlays.json` and comes back when the song opens. Mute in song is now a cell write.

| Piece | What it does | Where |
|---|---|---|
| `values` | The path rule (MON-3) over the sites. Mute, and mute in song, win over solo. | `crates/server/src/graph/overlay.rs` |
| `Monitor` | The overlays of the open project, in `Published` (M6): the sets, the accepted revision's tracks, sites and muted, whether this window may write. | same |
| `Graph::overlay` | Applies a change, writes the cells of what plays under the control lock (at most two cell writes), answers with the state. Refuses `song`, and a track that isn't there, in words. | same |
| `Graph::save_overlays` | Writes the file if it changed and the window may write: from the `music2-tell` thread, off the lock (M2). | same; one line in `crates/server/src/lib.rs` |
| `Graph::forget_overlays` | Drops keys (Q7), for taste-next's track delete and add at the merge. | same |
| `state.overlays` | `mute`, `solo`, `muted_in_song`, `heard` per track, `saved`. `feeds` comes in B4. | `Inner::state` (one line) |
| New plans | Every renderer gets `set_overlays` before it is sent: in `settle`, and the first one at open (A6). | `crates/server/src/graph/actor.rs` |
| Mute in song | `reach_to` treats a delta whose only module changes are `Muted` as reachable (A1: `structural()` unchanged); `value_writes` ignores `Flat.muted`; the `Reach` branch writes the overlay cells that changed. | same |
| Renames | A `Rekeyed` change renames the entries, and undo renames them back. The delta is only computed while some mute or solo is set. | same |
| Export | `overlays.active` (a Warning) when any monitoring mute or solo changes what is heard. | `Graph::export`, `overlays_warning` |
| `PlaysView` | Gains `info` (the plan: its overlay sites and cells) and `muted` (S6, plus one field). | `crates/server/src/graph/lease.rs` |
| Routes | `crates/server/src/routes_8b.rs`, its own router and read-only layer (G5); one `.merge` line in `routes.rs`. | |

**Tests** (`crates/server/src/graph/overlay_tests.rs`, `crates/server/src/routes_8b_tests.rs`, new files):

| Test | Red first? |
|---|---|
| `solo_keeps_a_bus_fed_and_its_sources`, `mute_wins_over_solo`, `a_track_muted_in_song_stays_silent_when_soloed` | — |
| `overlays_persist_in_music2` (written by the thread, not the request; reopened, the first renderer has them) | red without A6 |
| `a_read_only_window_keeps_overlays_in_memory` | — |
| `the_song_module_cant_be_muted` (and a missing track; exclusive; clear) | — |
| `a_new_plan_takes_the_overlays_before_it_is_sent` | — |
| `a_mute_in_song_toggle_is_a_cell_write` (no new plan; undo too) | red with `Muted` still structural |
| `a_rekeyed_track_keeps_its_mute` | — |
| `an_export_is_byte_identical_with_overlays_active` (only that one warns) | — |
| `mute_and_solo_answer_with_the_overlay_state` (over HTTP: the token, the words, the file) | — |

**A B2 bug B3's tests found** (fixed in its own commit, `0e39f1b`): when the song's output reads a track and the
output's edge doesn't change, placing its overlay made `place` think the output had a fade, and an edit with a fade
elsewhere panicked (`a_track_renamed_and_a_knob_automated_play_and_undo`). Whether the output fades is now decided
first. `an_edit_while_muted_keeps_the_track_silent` gained that case (red first: the panic) and one where the
track's export changes.

**Choices made here:**
- A track that isn't in the song can't be muted or soloed from the route; it can be unmuted, so a stale entry can
  be cleared.
- `saved` in the state means "this window saves them" (false when read-only), for the pill's tooltip.
- `heard` is `silenced` only for a track whose every site is off by a solo. A track with no sites is `heard`.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-server`; `m2-server` tests.

**Contract:** MON-3 (the path rule, mute wins) and its *Today*, MON-4 (the file, renames, read-only) and its
*Today*, MON-5 *Today*, RENDER-26 *Today* (`Muted` is no structure). The DIAG row `overlays.active` was already
in the table (M8).
