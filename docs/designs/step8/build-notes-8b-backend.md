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
| B4 | Mute in song as a builder: `mute_in_song`, `feeds`, `POST /api/graph/track/mute`, `m2 mute-in-song` | built |
| B5 | Gate 8's ducking clauses on `env` and `follow`; live equals offline on them | built |
| B6 | Clip gestures by id: create, move, resize, duplicate, paste, delete, set; `POST /api/graph/clips`, `m2 clips` | built |
| B7 | Split, join, quantize, a clip's notes and points; each clip's `sha` in the timeline | built |
| B8 | Automate names lanes after the control; Remove automation (`POST /api/graph/automate/remove`, `m2 unautomate`); lane-driven inputs in the view | built |
| W2 | Markers | handed to taste-next (S7) |
| B9 | Audition in the engine: live notes on a lane (`Msg::Live`), `Player::live`, how many are held | built |
| B10 | Audition on the server: the WebSocket and `POST /api/graph/audition`, sources, the watchdog, `live` in the status | built |
| B11 | Meters in the engine: taps on the output and each track; the callback's time; whether the engine idles | built |
| B12 | Meters on the server: the 30 Hz thread, the `meters` message on its own channel, `load` in the status | built |
| W1 | The keyed compressor and the gate's third clause | waits: std 0.2 has no keyed compressor |
| M1 | Main's sounds merge (std 0.2) merged in | merged |

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
| Window overlays | A fade's old side, and a ghost's input, that read an overlay before read one for the window (A3). No cell; the new plan's value for the same track and consumer, or with none, the old plan's gain there (review R1). | `core/runtime/src/overlay.rs` |
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
- What the overlay state reads of the song (tracks, sites, muted, feeds) is worked out off the control lock, and
  only when the code changed, so a knob drag never waits for it (a follow-up commit after B5).

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-server`; `m2-server` tests.

**Contract:** MON-3 (the path rule, mute wins) and its *Today*, MON-4 (the file, renames, read-only) and its
*Today*, MON-5 *Today*, RENDER-26 *Today* (`Muted` is no structure). The DIAG row `overlays.active` was already
in the table (M8).

---

## B4. Mute in song as a gesture

**In short:** one call mutes tracks in the song, or unmutes them: it writes `muted: true` into each `track()` call's
options, as one undo step. The state also says what each track still drives when muted (the Duck on the bass).

| Piece | What it does | Where |
|---|---|---|
| `mute_in_song(s, modules, muted)` | One `Confined` Tx: each edited range (or the point after the key), each module's `Muted`. The planner asserts each track is as asked. "Kick → Kick, muted in song"; "Kick and Bass". A track already so is skipped; with none left, nothing is written. | `crates/editor/src/mute.rs` |
| The option edit | Its own small edit over `sites::properties` (G7): append `muted: true`, replace its value, or take it out (and the object when it held nothing else). `track(k, fn)` gets `, { muted: true }` after the key. | same |
| `feeds(rev)` | For each track: each module and component that reads it through a sidechain input or a control export. A node in `song` is named `song` (Q6). | same |
| `state.overlays.feeds` | The feeds, for the strip's "Still drives Duck on Bass". | `crates/server/src/graph/overlay.rs` |
| `POST /api/graph/track/mute` | `{session, modules, muted}` → `{file, before, after, revision}`. In `WRITES_8B`: 423 in a read-only window. | `crates/server/src/routes_8b.rs` |
| `m2 mute-in-song <project> <track>… on\|off` | Takes the lock. | `apps/m2/src/edits.rs` (new), one arm and the usage lines in `main.rs` |

**Tests:**

| Test | Where |
|---|---|
| `mute_in_song_inserts_and_removes_the_option` (to the byte; the render changes and comes back to the bit) | `crates/editor/src/mute_tests.rs` |
| `every_way_a_track_call_is_written` (no options; `muted: false` replaced) | same |
| `several_tracks_mute_in_one_tx_and_undo_puts_the_options_back` | same |
| `the_song_and_missing_tracks_are_refused` | same |
| `feeds_name_the_duck` (first-song: the kick feeds `song`'s Duck) | same |
| `mute_and_solo_answer_with_the_overlay_state` (now also mute in song over HTTP) | `crates/server/src/routes_8b_tests.rs` |
| `mute_in_song_is_refused_read_only` (423, nothing written; mute while listening still works) | same |
| `mute_in_song_writes_the_option_and_takes_the_lock` | `apps/m2/tests/mute.rs` (new; `lock.rs` untouched) |

Written with the code; each passed on its first run. The server's red checks were in B3.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-editor`, `m2-server`, `m2`; their tests.

**Contract:** WRITE-10 and WRITE-18 (a bullet each, "From 8b.4", not inside the shared table or list), MON-2
*Today* (`feeds`), `docs/gaps.md` #4.

---

## B5. The ducking gate

**In short:** muting the kick keeps the bass ducked, whether the duck reads `kick.env` or `follow(kick.out)` in the
song. Proven sample for sample on two new fixtures.

| Piece | Where |
|---|---|
| Fixtures `env` and `follow`: a kick on every beat, a bass drone ducked to it, std's mixer | `core/session/tests/fixtures/sidechain/` (README updated) |
| The gate and its control | `core/play/tests/mute.rs` (G3: no `crates/gates` change) |

**Tests:**

| Test | What it compares | Red first? |
|---|---|---|
| `muting_the_kick_keeps_the_bass_ducked` | the kick muted in the song = the kick taken out of the mix with its sidechain kept, as `f32`s (A9) | red on `follow` with the checker's sidechain rule turned off |
| `without_the_sidechain_the_bass_changes` | the control: muted, and the duck reading 0 instead, differs | — |
| `a_monitoring_mute_of_the_kick_live_is_its_mute_in_song` | the kick muted while listening, live through the driven player = muted in the song, offline | — |

**A change from the plan:** the plan's reference was "the kick's channel at −96 dB". std's `gain` doesn't
treat −96 dB as silence (10^(−96/20) is about 1.6e-5), so that render can't equal a muted kick. The reference
here is the song with the kick's channel taken out of the mixer, its sidechain still read. That is exactly what
MON-2 promises.

**Checks:** `m2-play` tests; clippy on `m2-play`.

**Contract:** gate 8's row says which of 8b's clauses pass. The keyed compressor (W1) waits for std 0.2.

---

## B6. Clip gestures by id

**In short:** clips can now be made, moved (to another lane too), copied, resized from either
edge, duplicated, pasted and deleted, each as one edit that undo takes back. One request type
does it all, for the app, the CLI and agents.

| Piece | What it does | Where |
|---|---|---|
| `Gesture` | `create`, `move` (by beats; onto lane `to`; `copy`), `resize` (start and length), `duplicate`, `paste`, `delete`, `set` (whole clips put and ids deleted, for agents). JSON tagged by `op`. | `crates/editor/src/clips.rs` (new) |
| `ClipRef` | A clip by `{module, lane, id}`, or by `at: [start, rank]` for a clip with no usable id. With `sha`, a clip changed since is refused (the timeline gives each clip's `sha` in B7). | same |
| `apply(rev, g, mint)` | Pure: what the gesture makes of each lane it touches, from the accepted revision. The lane lease can call it to preview. | same |
| `edit(s, g)` | One Tx: a `Clips` op per lane (the whole lane when its clips can't all be named by id), `Confined { lanes }`, the planner's assertion. Answers the clips it made, to select. | same |
| `position(arr, "9.3.1")` | bar.beat.sixteenth to beats, by the song's meter. | same |
| `Graph::clips` | Through `Actor::edit_and`: the data path, heard before durable. 409 when a clip named by its `sha` changed. | `crates/server/src/graph/clips.rs` (new) |
| `POST /api/graph/clips` | `{session, gesture}` → `{file, before, after, revision, made: [{module, lane, id}]}`. In `WRITES_8B`. | `crates/server/src/routes_8b.rs` |
| `m2 clips` | `create`, `move`, `resize`, `duplicate`, `delete`, and `json` for any gesture. Clips as `track/lane:id` or `track/lane@9.3.1`. Takes the lock. | `apps/m2/src/edits.rs` |

**The rules, as built:**

| Rule | How |
|---|---|
| ids | A move keeps the id; onto a lane that has it, a fresh one. Copies, duplicates, pastes and new clips get fresh ones. Every clip of a lane written gets an id (FMT-12). |
| kinds | Notes, automation and audio never cross. Automation crosses only within its unit: "Bass's cutoff lane (hz) and Bass's drop lane (unit01) can't share clips". |
| create | One bar at the meter there. Automation holds the lane's value at its start (the clip there, else the fallback). Audio refuses: there's no empty sound. |
| left edge | Notes and points shift the other way, so they play where they did; notes before the edge stay in the file, silent. Trimming back gives the same bytes. An audio clip's offset moves by the seconds the edge moved, and stops at the asset's start. A pitched or reversed audio clip refuses (A8); a warped one shifts its markers. |
| duplicate | Lands after the selection: a span of a bar or more rounds up to whole bars (31 beats lands on bar 9, 33 on bar 10); shorter rounds up to `grid`, or not at all. A marquee's `range` is the span. |
| paste | Offsets from the earliest copied clip kept. Onto lane `to` when the clips are of one kind; else each onto its own lane. |

**Tests** (`crates/editor/src/clips_tests.rs`, `crates/server/src/graph/clips_tests.rs`, `apps/m2/tests/clips.rs`, all new; one in `crates/server/src/routes_8b_tests.rs`):

| Test | Red first? |
|---|---|
| `a_move_keeps_its_id` (one line changes; onto another lane; undo to the byte) | red against the stub (all 14 editor tests were) |
| `a_move_onto_a_taken_id_mints_one`, `every_copy_mints` (copies play what the original plays) | red against the stub |
| `a_left_trim_keeps_notes_in_place_and_trimming_back_brings_them_back` | red against the stub |
| `an_audio_left_trim_moves_its_offset` (`examples/resample`; the asset's start; pitch and reverse refused) | red against the stub |
| `a_new_clip_is_a_bar_long_and_automation_holds_the_lanes_value` | red against the stub |
| `a_duplicated_section_lands_on_the_next_bar`, `a_marquee_duplicate_keeps_its_empty_bars` | red against the stub |
| `a_paste_mints_and_keeps_offsets`, `kinds_and_units_never_mix` | red against the stub |
| `touching_a_lane_gives_its_anonymous_clips_ids`, `delete_and_set`, `apply_is_pure`, `a_first_clip_makes_the_lane_file` | red against the stub |
| `places_are_bar_beat_and_sixteenth` | — |
| `a_clip_gesture_takes_the_data_path` (no QuickJS run, the same renderer) | — (written after the code) |
| `clip_gestures_over_http` (the token, words, 422, 423 read-only) | — (written after the code) |
| `clip_gestures_from_the_terminal` (by id and by place, refusals, the lock) | — (written after the code) |

**Choices made here:**
- **`sha` rides in each `ClipRef`**, not in a separate `seen` map: ids repeat across lanes once a
  move keeps one, so a map by id can't say which clip it means.
- **`made` lists clips put on a lane they weren't on:** new ones, copies, and moves to another
  lane. A move along its lane keeps its id, so the app knows it already.
- **A lane only loaded to check** (a move's target that ends up unchanged) is not written, so its
  anonymous clips keep no new ids.
- **Positions in the CLI** are bar.beat.sixteenth with quarter-note beats, as the timeline counts.
  `--by`, `--length` and `--grid` are beats.
- **Labels** reuse the piano roll's words for notes lanes ("clip at bar 1 (8 bars)"). Several lanes
  are named in turn: "Kick hits: …; Hats hits: …".
- **Errors** are 400 like every edit route, except a stale `sha`, which is 409 with `changed: true`.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-editor`, `m2-server`, `m2`; tests
of `m2-editor`, `m2-server`, `m2`.

**Contract:** WRITE-10 and WRITE-18 (a bullet each, "From 8b.5"), FMT-12 (ids for the lanes a builder
writes, and its *Today*).

---

## B7. Split, join, quantize, notes and points; the clip's hash

**In short:** a clip can now be split and joined back, its notes quantized, its notes or its
points drawn. Each clip in the timeline carries a hash, so a gesture on a clip that changed since
the window drew it is refused, naming the clip.

| Piece | What it does | Where |
|---|---|---|
| `split` | At a beat, every picked clip it crosses. Notes that cross are shortened; the rest go to a new clip, in place. Automation ends and starts at the value at the split. An audio clip's second half starts later in its asset (A8: not when pitched or reversed). | `crates/editor/src/clips.rs` |
| `join` | Notes clips of one lane, as one: earliest start to latest end, the earliest's id. Every note plays as it did (below). Automation and audio refuse, in words. | same |
| `quantize` | Note starts to the nearest grid line, ends too with `ends`; `notes` picks notes of one clip. | same |
| `notes`, `points` | One clip's notes (and length), one clip's points: the piano roll's edit and automation drawn in place. | same |
| `sha` per clip | The first 16 hex of the SHA-256 of the clip's canonical line, added to each clip of a rebuilt lane's JSON (G6, one call). | `clips::with_sha`; one call in `actor.rs`'s `timeline()` |
| Seen | A `ClipRef` with a `sha` the clip no longer has is `Changed`: "data/kick/hits.json (clip `a`) is the Kick clip at bar 1, which was changed in another window or by an agent. It shows as it is now; try again". 409 at the route. | `clips.rs`, `graph/clips.rs` |
| The roll's lane write | Reads clips as the timeline shows them, `sha` and all (Q9: it stays until the roll sends `notes`). | `routes_8b::shown_clips`; one attribute in `routes.rs` |
| `m2 clips split\|join\|quantize` | `--at <place>`; `--grid B [--ends]`. | `apps/m2/src/edits.rs` |

**Join keeps every note in time.** A note keeps its place. One a clip's edge hid only partly, and
that the joined clip would now let through, is cut to what played; one hidden whole that would now
play is dropped. A note that played whole, or stays as hidden as it was, is kept whole. So a split
and a join play exactly what played (`join_makes_one_clip_and_keeps_every_note_in_time`).

**Tests:**

| Test | Where | Red first? |
|---|---|---|
| `a_split_shortens_the_crossing_notes`, `a_split_control_clip_stays_continuous` (every quarter beat the same) | `crates/editor/src/clips_tests.rs` | red against the stub |
| `a_split_audio_clip_plays_the_same_samples` (the resample song, to the bit; pitch and reverse refused) | same | red against the stub |
| `join_makes_one_clip_and_keeps_every_note_in_time`, `control_and_audio_clips_dont_join` | same | red against the stub |
| `quantize_to_sixteenths_and_triplets`, `notes_and_points_edit_one_clip` | same | red against the stub |
| `with_sha_gives_each_clip_its_hash` | same | red against the stub |
| `a_gesture_on_a_changed_clip_is_changed`, `undoing_after_an_outside_change_to_the_clip_conflicts` | same | — (the check came with B6's `resolve`) |
| `an_outside_edit_to_another_clip_merges` | same | **red: a B6 bug** (below) |
| `the_timeline_carries_each_clips_sha_and_a_stale_one_is_refused` (not taken in: the commit's conflict; taken in: the stale `sha`) | `crates/server/src/graph/clips_tests.rs` | red before `timeline()` added it |
| `the_rolls_lane_write_takes_clips_as_the_timeline_shows_them` | `crates/server/src/routes_8b_tests.rs` | red: 422, `sha` an unknown field |
| `split_join_and_quantize_from_the_terminal` | `apps/m2/tests/clips.rs` | — (written with the code) |

**A B6 bug B7's tests found** (fixed in its own commit, `dd3e44c`): the planner's assertion checked every clip of a lane, so an outside
edit to another clip, which WRITE-3 merges, made the gesture refuse ("would change more than its
clips"). It now checks the clips put and deleted only, as the notes write does.

**Choices made here:**
- **A split with no picked clip crossing it** says so; picked clips it doesn't cross are left.
- **Quantize from the song's start:** grid lines are multiples of `grid` beats from beat 0. A clip
  that starts on the grid gets exact multiples, so sixteenths stay short numbers in the file.
- **A warped audio clip** splits and trims by moving its markers, which keeps its timing exact;
  `offset` isn't read with warp.
- **`sha` in the timeline only:** `m2_editor::timeline()` is unchanged (taste-next edits it); the
  server adds it where it rebuilds a lane's JSON, so an unchanged lane costs nothing. The test that
  compares the published timeline with the session's leaves `sha` out.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-editor`, `m2-server`, `m2`; tests
of `m2-editor`, `m2-server`, `m2`.

**Contract:** WRITE-10 (the 8b.5 bullet names every gesture), WRITE-12 *Today* (the clip hash; the
roll's write stays), `docs/gaps.md` #9, #14, #17.

---

## B8. Automation: names, Remove, and what a lane drives

**In short:** Automate now names a lane after its control: `decay`, not `out-decay`. Remove
automation takes a lane out again and writes back what it played. The canvas data says which lane
drives an input.

| Piece | What it does | Where |
|---|---|---|
| Lane names (D6, M7) | The key is the control's label as a key (`decay`, `detune-cents`); a mixer channel's control stays `hats-pan`. On a clash, the call's binding (else its component) goes in front (`pad-decay`), then a number. `{ name: "Detune (cents)" }` is written only when the key, read as words, isn't the label. | `crates/editor/src/automate.rs` (`name_of`) |
| `automate::remove(s, module, lane, beat)` | The lane's read becomes what it plays at `beat` (the playhead), in its fallback's spelling (`hz(1250)`), or the fallback as written when `beat` is none; the lane file goes. One `Confined` Tx; undo puts both back. | same |
| `POST /api/graph/automate/remove` | `{session, module, lane, beat?}` → `{file, before, after, revision}`. In `WRITES_8B`. | `crates/server/src/routes_8b.rs`, `graph/clips.rs` |
| `m2 unautomate <project> <track>/<lane> [--at <place>]` | G9: its own command, not a flag on `automate`. Takes the lock. | `apps/m2/src/edits.rs` |
| `InputPort.lane` | `{module, lane}` on every input a control lane drives, cabled or not. Left out where none does. | `crates/editor/src/view.rs` |

**Tests:**

| Test | Where | Red first? |
|---|---|---|
| `the_lane_is_named_after_the_control` (the name written only when needed; a clash) | `crates/editor/src/automate_tests.rs` | red before `name_of` changed |
| `an_inherited_control_is_written_into_its_call_as_a_lane`, `each_way_a_function_takes_its_context_reaches_the_lane` (now `decay`, `cutoff`) | same | red before `name_of` changed |
| `the_new_lanes_key_is_free` (a clash puts the binding in front) | same | — (passed on the old names too) |
| `remove_automation_puts_the_heard_value_back` (at a clip, past it, the fallback; undo; automate then remove) | same | red against the stub |
| `remove_automation_refuses_what_it_cant_take_out` | same | red against the stub |
| `a_lane_read_by_name_is_taken_out` | same | — (written after the fix the CLI test showed) |
| `an_input_a_lane_drives_names_the_lane` | `crates/editor/src/view_lane_tests.rs` (new) | red with the field always none |
| `remove_automation_over_http` (the words; 423 read-only) | `crates/server/src/routes_8b_tests.rs` | red against the stub |
| `unautomate_from_the_terminal` (`--at`, a mask, a notes lane, the lock) | `apps/m2/tests/clips.rs` | **red: the mask was refused** (below) |
| `a_track_renamed_and_a_knob_automated_play_and_undo` (now `freq`) | `crates/server/src/graph/tests.rs` | updated |

**What the CLI test found:** removing `const drop = lane.mask("drop")` was refused by `Confined`: the
plain number written in its place is made where `blend(…, drop)` reads the name, outside the read's
range (PKG-3). Remove now also declares each place the file reads the binding by name. Nothing is
written there.

**Choices made here:**
- **`name` in automate's answer stays the knob's words** (`decay`, `hats pan`), as the toast uses
  it; the lane shows them as a label, first letter a capital.
- **The fallback's spelling** is kept: a unit call stays that call (`khz(1.25)`), a plain number
  stays a number; with no fallback, the unit's own call, imported if needed.
- **A lane read in several places** (a shared lane), or outside the song's own code, is refused in
  words: take it out in the code.
- **`e2e/s6-builders.mjs`** expects `hi`, not `wobble-hi` (M7). Run at the End.
- **No test of "a lane-driven knob refuses a drag":** the back end has no knob there to drag; the UI
  reads `InputPort.lane`. Latches stay MON-6's.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-editor`, `m2-server`, `m2`; tests
of `m2-editor`, `m2-server`, `m2`.

**Contract:** HOME-2 (its 8b.5 *Today*), WRITE-10 and WRITE-18 (the 8b.5 bullets), `docs/gaps.md`
#14. README's automate line.

**Markers (W2):** handed to taste-next, as S7 decides: they live in its `layout.rs`. Nothing here.

---

## B9. Live notes in the engine

**In short:** a key held in the app can now sound in the engine. A lane plays a note that isn't
in its data, from the next block. It keeps sounding through stop, seek, loop wraps and edits,
until it is let go. Nothing offline changes.

| Piece | What it does | Where |
|---|---|---|
| `LiveEv` | `On { source, handle, pitch, velocity }`, `Off { source, handle }`, `Release { source }`. A source is a window; it names its own notes by handle. | `core/runtime/src/sched.rs` |
| The live table | Each notes lane's scheduler holds up to 16 live notes, each with an engine id. The 17th On is refused and counted with the dropped Ons. | `NoteScheduler::live` |
| Merging | Each block sends the live Ons and Offs first, at frame 0, before the song's notes, playing or stopped. A jump or a swap never touches them. | `NoteScheduler::run` (`send_live`) |
| Edits | The table moves with its lane's scheduler at an activation, into a grown one too (`take_state`). A replaced consumer's notes end with its ghost (S5). | `sched.rs` |
| Panic | Clears the table without Offs (A10): the voices are reset. | `NoteScheduler::reset` |
| `Renderer::live`, `live_notes` | Takes a note for a slot; a release reaches every lane. Refuses a slot it doesn't have, a pitch outside 0 to 127, a velocity outside 0 to 1 (R2). `can_idle` is false while any is held. | `core/runtime/src/lib.rs` |
| `Msg::Live { generation, slot, ev }` | In order behind activations. Another plan's is refused and counted (`Engine::refused_live`). Needs no stash room (M4). `Status.live`. | `core/runtime/src/engine.rs` |
| `Player::live`, `live_notes` | Sends it; the count is published after each callback. | `core/play/src/lib.rs` |
| Room | Each consumer has room for 32 more held notes (a poly's plan, a plugin's keys and events). | `Builder::node`, `Builder::plugin` |

**Tests** (`core/runtime/src/tests/audition.rs`, new; `core/play/tests/no_alloc.rs`):

| Test | Red first? |
|---|---|
| `a_live_note_sounds_and_ends` | red against stubs (all 11 below were) |
| `live_ids_never_meet_scheduled_ids` (the probe sees no repeated id, no unknown Off) | red against stubs |
| `a_live_note_survives_stop_seek_and_wrap` | red against stubs |
| `a_live_note_moves_with_its_lane_across_an_activation` (kept, and grown) | red against stubs; red again with `take_state` not copying the table |
| `a_replaced_consumer_ends_its_live_notes` | red against stubs |
| `a_key_struck_and_let_go_at_once_still_ends` | red with the On and Off sent in one block (the note stayed on) |
| `a_full_table_refuses_and_counts`, `release_ends_one_sources_notes`, `panic_ends_live_notes` | red against stubs |
| `a_live_note_wakes_an_idle_engine`, `another_generations_live_is_refused` | red against stubs |
| `held_for_an_export_an_on_is_refused_and_an_off_waits` (M4) | red against stubs |
| `live_notes_never_allocate` (voices and a plugin) | red with the panic's reset allocating a new table |

**Choices made here:**
- **Held for an export (M4):** the plan said "refused and counted". Only an On is refused. An Off
  or a release is kept and sent at the resume. Refusing an Off would leave the note on for good.
- **A key struck and let go at one block line** sends its On then and its Off at the next block.
  At one frame an Off sorts before an On, which would leave the note on.
- **Room:** the table has 32 places, and 16 notes may be held. So a key let go and another struck
  at one block line (a pitch drag) never refuse each other.
- **Ids are given when the message applies**, not in `run()`. Both happen at the same block line,
  so the order of ids is the same.
- **A release names no lane:** it ends the source's notes on every lane.
- After an export that borrowed a plugin, the plugin starts its live notes again at the resume, as
  it does the song's (`sounding_note` knows them). Not tested here.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-runtime` and `m2-play`; tests of
`m2-runtime`, `m2-play`, `m2-session` (`renders.json` unchanged: offline renders never get a live
note).

**Contract:** MON-7 (the live note, its source, room, ids, what ends it) and its *Today*, RENDER-9
(item 1: notes a lane scheduled), RENDER-14 (merging at frame 0, and its *Today*), RENDER-26a,
RENDER-28 (the message list, and its *Today*), the Engine shape.

---

## B10. Audition on the server

**In short:** a window can now play notes. It sends them on its WebSocket; an agent sends them to
`POST /api/graph/audition`. They never wait for the session, and work in a read-only window.
They end when let go, when the window closes, or when it goes quiet for 10 s.

| Piece | What it does | Where |
|---|---|---|
| `Graph::audition(client, msg)` | `{module, lane, on: [{h, pitch, velocity}], off: [h], keep: [h]}`. Offs first, then Ons. Finds the lane in the plan last sent and sends `Live` under the control lock, so it queues behind that plan's activation. Refuses a pitch outside 0 to 127, a velocity outside 0 to 1, and notes with no lane, in words (R2). | `crates/server/src/graph/audition.rs` (new) |
| Sources | Each window (`client`) is a source, numbered on first use. Each held handle remembers its lane by name, so an Off after an edit finds it in the new plan. A handle struck again is let go first, on whatever lane it sounds. | `Sources` in one `ControlPlane` field |
| A lane nothing plays | Answers `{"t": "audition", "module", "lane", "silent": "Nothing plays this lane"}`, to that window only. | same |
| The window closes | `client_gone` lets go of its notes (one release). | one line in `Graph::client_gone` |
| The watchdog | A window quiet for 10 s is let go and forgotten. Run by `music2-tell` every 300 ms. | `Graph::expire_auditions`; one line in `crates/server/src/lib.rs` |
| `POST /api/graph/audition` | `{client, …}`, not in `WRITES_8B`. Answers `{}` or the silent answer. | `crates/server/src/routes_8b.rs` |
| The WebSocket | `ws_loop` reads text now. `{"t": "audition", …}` goes to `routes_8b::ws_text`; its answer, or `{"t": "audition", "error": …}`, goes to that socket only. | `crates/server/src/routes.rs` (a few lines) |
| `live` | Live notes sounding, in `/api/graph/status` and the graph state. | `Activity.live` |

**Tests** (`crates/server/src/graph/audition_tests.rs`, new; `crates/server/src/routes_8b_tests.rs`):

| Test | Red first? |
|---|---|
| `a_note_sounds_until_it_is_let_go` (a pitch drag in one message) | — |
| `a_handle_struck_again_elsewhere_lets_go_first` | red without it: two notes left on |
| `a_lane_nothing_reads_says_so` (an unread lane, and a missing one) | — |
| `bad_notes_are_refused_in_words` | — |
| `a_closed_window_releases_its_notes` (only its own) | red without the hook in `client_gone` |
| `the_watchdog_releases_a_quiet_window` (a keepalive keeps the other) | red with the wrong quiet time |
| `audition_works_read_only` | — |
| `an_off_after_an_edit_finds_its_lane` (the lane's slot moves) | red with the Off sent to the old slot |
| `audition_never_waits_for_a_compile`: sent in 0.01 ms, heard 1247 ms before a 1262 ms compile ended (Linux, information only) | — |
| `audition_over_http_and_the_websocket` (the token; a window's answers reach only it; its close lets go) | red with `ws_loop` ignoring text |

The tests were written after the code here, so most were made red by breaking the code they
check, as the column says.

**Choices made here:**
- **A WebSocket test without a new crate:** a small client written by hand in the test (the
  RFC's framing). Adding `tokio-tungstenite` would change `Cargo.lock`.
- **A window without a `client` id** can't audition; it is told so. The web always sends one.
- **`silent` names the lane** too, so the roll can tell which lane it was about.
- **`velocity` is 0.8 when left out**, for agents.
- The answer to "a lane nothing reads" comes for every message with Ons. The roll shows it once.

**Seen once, not ours:** in one full `m2-server` run, `loading_is_published_while_a_plugin_is_created`
failed, and passed in the next run and 5 runs alone. It reads the loading list twice, from the
activity and then from the state, and the load can end between the two reads while the machine
is busy. Step 7's test; left as it is.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-server`; `m2-server` tests.

**Contract:** MON-7 *Today* (the server half), RENDER-29 *Today*, `docs/gaps.md` #8. The Planes
table already lists audition in the control row.

---

## B11. Meters in the engine

**In short:** a live plan now measures itself. After each block it keeps the loudest sample of
the song's output and of each track. Nothing it plays changes.

| Piece | What it does | Where |
|---|---|---|
| `TapSlots` | One atomic slot per channel: the output's two, then each track's. `take()` reads them and sets them back to 0. | `core/runtime/src/taps.rs` (new) |
| `Taps` | The audio thread's side. After each block, each channel's largest `\|x\|` is raised into its slot with `fetch_max` (a non-negative `f32` orders as its bits do). No lock, no allocation. | same |
| What is tapped | The output, and every overlay node with a cell, by its track (S4: fixed when the plan is built, no `set_taps`). A track read in two places is one meter, the larger. | `Renderer::build` |
| `PlanInfo.taps` | The slots, for the server. Only a live plan (`Renderer::prepare`) has them; `new`, `lending` and `starting_at` don't. | `core/runtime/src/plan.rs` |
| `Player::load()` | The DSP load (Q3, C2): the callback times itself with the clock alone. `longest` is the longest callback since the last read, as a share of its period; `late` counts callbacks longer than their period. Reading it starts `longest` again, so the meters thread alone reads it (M3). | `core/play/src/lib.rs` |
| `Player::idle()` | Whether the engine renders nothing: idle (RENDER-6a), or no project plays. Published after each callback. | same |

**Tests** (`core/runtime/src/tests/taps.rs`, `core/play/src/load_tests.rs`, new; `core/play/tests/no_alloc.rs`):

| Test | Red first? |
|---|---|
| `taps_change_no_bit` (a live plan against an offline render, to the bit) | — |
| `a_tap_reports_its_blocks_peak` (over 1, 3 and 7 blocks; taken, then 0) | red before the block wrote them |
| `a_muted_tracks_tap_reads_silence` (muted in the song, and while listening) | red before the block wrote them |
| `the_load_is_the_longest_callback` (a 1 GHz device makes every callback late; at 48 kHz never more than the test's own clock saw) | red before the callback timed itself |
| `the_engine_says_when_it_idles` (no project, stopped and silent, playing silence, a sine ringing on stopped, unloaded) | red before it was published |
| `taps_and_the_load_never_allocate` (another thread reads and resets the taps and the load meanwhile) | — |

**Choices made here:**
- **Where a track is tapped:** at its overlay, after its mute (Q2). So a muted track's meter
  falls to the floor. Post-fader meters come with 8b.3's channel gains.
- **A sample that isn't finite:** a NaN is passed over; an infinity reads as the largest `f32`.
  So a meter never holds a value JSON can't carry.
- **A mono node in a stereo track** gives its one channel to both.
- **A track nothing reads has no meter:** it has no site, and isn't heard either.
- **`late` counts for good,** from the player's start, like `xruns`. A window that missed a
  meters message still sees a dropout as a larger count.
- **The callback is timed from its first line to its last,** after it published, so the time
  includes everything it did.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-runtime` and `m2-play`; tests
of `m2-runtime`, `m2-play`, `m2-session` (`renders.json` unchanged: offline renders have no taps).

**Contract:** the Engine shape (`taps`), RENDER-28 *Today* (the timer, `idle`).

---

## B12. Meters on the server

**In short:** every window now gets the meters about 30 times a second while the song plays:
each track's peak, the song's output, and the DSP load. Nothing is sent while the engine idles.

| Piece | What it does | Where |
|---|---|---|
| `Graph::meters()` | Reads and resets the taps of the plan that plays, and reads the load. `None` while nothing plays, while the engine idles (but one last message as it starts to), and while a new plan waits to be taken. | `crates/server/src/graph/meters.rs` (new) |
| The message | `{"t": "meters", "tracks": {"kick": [l, r], …}, "out": [l, r], "dsp": {"load": 0.23, "dropouts": 0}}`. Peaks linear, to a millionth. | same |
| `Meters` | The last 30 loads (a second), and whether the engine idled at the last read. One `ControlPlane` field. | same; one line in `graph.rs` |
| `music2-meters` | A thread: every 33 ms, `meters()`, sent with `send_replace` on `AppState.meters`, a `watch` channel (M1). It reads the load even with no window, so the status stays current; it sends only while a window listens. | `crates/server/src/lib.rs` |
| `ws_loop` | Also waits on the meters channel, and sends the latest. | `crates/server/src/routes.rs` (one branch) |
| Status | `load` (the longest callback of the last second, as a share of the period) and `late`, from the meters' reads (M3). In the graph state too. | `Activity` |
| Taps per plan | Nothing to add: `PlaysView.info` is the plan's `PlanInfo`, which holds its taps (S4, S6). | — |

**Tests:**

| Test | Where | Red first? |
|---|---|---|
| `meters_are_sent_while_playing_and_stop_when_idle` (idle at open: one last, then none; playing; stopped: rings, then the last reads silence) | `crates/server/src/graph/meters_tests.rs` (new) | red with no last message as it idles |
| `a_new_plan_moves_the_taps` (a track added; none while the plan waits) | same | red without the wait for the plan |
| `muting_the_kick_floors_its_meter_and_the_bass_still_ducks` (`env` fixture) | same | — (the runtime's muted-tap test was red) |
| `meters_reach_every_window_and_the_status_has_the_load` (two windows, the token, `load` and `late` in the status) | `crates/server/src/routes_8b_tests.rs` | red with `ws_loop` not reading the channel |

The tests were written after the code. Three were made red by breaking the code they check, as the column says.

**Choices made here:**
- **`out` has its own field,** not a key in `tracks`: a track may be called `out` (ID-1 allows it).
- **One last message as the engine starts to idle,** so meters fall to the silence it ended in,
  rather than freezing on the last loud read. Then none.
- **No message while a new plan waits to be taken:** its taps read nothing yet, so a message
  would flash every meter to the floor for one frame at each edit. The window keeps the last.
- **`dropouts` is a count since the player started** (xruns plus late callbacks), not new ones
  per message. A window may miss messages (the channel keeps the latest), so it lights its
  warning when the count grows.
- **The bass in the ducking test is metered before the duck:** the duck is written in the song,
  and tracks are tapped at their overlay (Q2). The output's meter shows the duck instead. With
  8b.3's channel gains, a track's meter moves after its fader.

**Checks:** `cargo check --workspace --all-targets`; clippy on `m2-server`; `m2-server` tests.

**Contract:** the Engine shape (the server's half), the Deferred table (taps and meters built;
freezing and bounce as a Tx stay), `docs/gaps.md` #20.

---

## M1. Main's sounds merge

**In short:** main (`1e5afe3`) is merged in. It brings std 0.2. Every test project here now asks for
std 0.2.

| What | How |
|---|---|
| `docs/architecture.md` | The one conflict: gate 7's row is main's (passed again with the sounds merge); gate 8's row keeps 8b's clauses. W1 now says what it waits for: std 0.2 has no keyed compressor. |
| Test manifests | `routes_8b_tests.rs` (7) and the `sidechain` fixtures asked for std 0.1. Main's `every_shipped_manifest_is_met` would fail on the fixtures. All now say `0.2`. |
| `std.wasm` | Unchanged on both sides: nothing to rebuild. |

**Checks:** tests of `m2-session`, `m2-play`, `m2-server`.

---

## Review fixes

**In short:** a review of the track found six things. Five are fixed here, each in its own commit
with a test that failed before it. One (R2) was a wrong doc line.

### R1. A window overlay with no new site keeps the old gain

**What was wrong:** in an activation's window, a fade's old side or a ghost's input gets its own
overlay. It took the new plan's value for the same track and consumer. When the new plan had no
such site, it started at 1. So a muted kick was heard for 5 to 10 ms when the edit deleted it, or
made a new bus track read it instead of the song. A kick muted in the song did the same once
deleted, as the new graph no longer says it was muted.

**The fix:** each window node remembers the old site it stands in for. At the block line,
`take_over` copies that site's gain (where it goes, and where it is) from the old plan's node. Two
`f32` copies; no allocation (`window_held` in the plan).

| Test | Red first? |
|---|---|
| `an_edit_while_muted_keeps_the_track_silent`: three new cases, the kick deleted (muted while listening, and in the song) and rerouted into a bus | red: peaks 0.325, 0.325 and 0.5 |

**Checks:** `m2-runtime` and `m2-play` tests; clippy on `m2-runtime`.

**Contract:** MON-1's window rule.

