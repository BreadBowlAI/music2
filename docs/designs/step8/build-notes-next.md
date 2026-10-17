# Build notes: next

The `next` track (plan: `/home/jerry/m2-runs/s8n/plan.md`). Each part writes under its own heading.

## P0

(The lead's.)

## Lane A

Hear it while you drag (8b.7): lane leases, then the tempo lease. Branch `next-a`.

**For 8a's merge:** lane A changed nothing in `core/runtime`, `core/play` or `core/os` (A4 isn't built, X3). In
`crates/server/src/graph/actor.rs` its blocks are `Plays.tempo`, `way()`'s tempo argument and its two lines,
`build()`'s `data` argument, `settle`'s first lines (the tempo drag read), the `Timing` map line, the two calls after
the engine view's publish, `settle_as`'s phase-2 return, and `tempo_preview`; none is in `render` or `export`, the
two functions 8a changes there. `core/song/src/data.rs` gains `with_arrangement` only.

### A1. Lane leases on the server

**In short**

| What | Now |
|---|---|
| A note, a clip or an automation point dragged while the song plays | The server can play it where it is, before you let go |
| Letting go | One write, one Undo step, as before. What you hear doesn't change |
| Esc, or the window closes, or nothing is heard from the drag for 10 s | What was there plays again. Nothing is written |
| Someone else changes the clip you drag | Your drag stops. The new clip plays. The window is told why |
| An audio clip | Not heard while dragging. It is written when you let go, as before |

The page doesn't use it yet: that is A2.

**What changed**

- `crates/editor/src/clips.rs`: `apply_as(rev, g, tx_id)` is `apply` minting ids as Tx `tx_id` does;
  `edit_as(s, g, tx_id)` is `edit` as Tx `tx_id` (`edit` calls it with a random id). So a copied clip has the same id
  in the preview and in the file (Q21). `pinned(rev, g)` fills in the hash of each clip a gesture names, from `rev`,
  where the gesture carries none: what a lease saw.
- `crates/server/src/graph/lanes.rs` (new, an `impl Graph` block, as `audition.rs` is): `lane_begin`,
  `lane_update`, `lane_keepalive`, `lane_cancel`, `lane_commit`, `lane_leases`; and three calls for the rest of the
  server: `settled` (the actor's settle), `expire`, `gone`, `end_all`.
  - begin checks the gesture on `EngineView.accepted`, never waiting for the session (RENDER-29). Only `move`,
    `resize`, `notes`, `points` and `create` are previewed; a gesture naming an audio lane answers
    `previewable: false` and makes no lease.
  - update applies the gesture with `apply_as` off the lock, then, under the control lock and only while the engine
    view is the one it applied on (else again, 4 tries), sends `SwapLane` for each lane it edits that the plan
    reads. A notes lane whose preview needs more room than its scheduler has is skipped (heard at the commit, whose
    activation grows it). A lane an earlier update reached and this one doesn't gets its accepted data back.
  - commit marks the lease closing and runs the clip gesture through the session as `POST /api/graph/clips` does,
    as Tx `tx_id` (`Graph::clips_as`, new in `graph/clips.rs`). Its PLANE-5 preview is the lease's preview, so
    nothing heard changes. Then the lease ends; a lane whose accepted data isn't what the lease played (a refused
    commit) gets the accepted data back.
  - cancel, TTL (10 s, `ttl_ms` up to 60 s), the window closing (`client_gone`) and the song closing end it, the
    accepted data played again.
  - `settled`, called by the actor's settle under the control lock after its own sends: for each live lease, a new
    plan (another generation) or new accepted data for a lane it names re-applies the gesture on the new revision
    and sends it, so it plays from the new plan's first block or the one after. A gesture that no longer applies (the
    dragged clip changed: its hash, pinned at begin) ends the lease; its next request hears
    "Bass changed elsewhere: your drag was dropped."
- `crates/server/src/routes_leases.rs` (new router): `POST /api/graph/lane/lease/{begin,update,keepalive,commit,cancel}`.
  begin `{session, gesture, client?, ttl_ms?}` → `{lease, previewable, lanes, heard, revision}`; update
  `{lease, seq, gesture}` → `{seq}`; commit `{lease, gesture}` → `POST /api/graph/clips`'s answer, 409 with
  `changed: true` for a changed clip. Its own write list and 423 middleware (X33): begin and commit.
- `graph.rs`: `EngineView.accepted` and `ControlPlane.lanes` at their structs' ends; `mod lanes;` at the list's
  end; one line each in `end_all`, `expire` and `client_gone`.
- `graph/actor.rs`: the settle's phase 1 publishes `accepted` with the engine view and calls `lanes::settled`
  right after (one block); `play()` publishes `accepted` too.
- `routes.rs`: the `.merge` line after `routes_view`'s, and `graph_leases` lists lane leases after the knobs', each
  with `kind: "lane"` (three lines in that function: `GET /api/graph/leases` is in `routes.rs`).
- `lib.rs`: `mod routes_leases;`.

**Tests** (`crates/server/src/graph/lanes_tests.rs`, a driven player, the renderer's trace read for each On and Off):
`a_lane_lease_previews_with_no_revision`, `the_commit_changes_nothing_heard`, `cancel_puts_the_accepted_notes_back`,
`a_closed_window_cancels_its_lane_lease`, `an_idle_lane_lease_expires`,
`an_outside_write_to_another_clip_keeps_the_preview`, `an_outside_write_to_the_dragged_clip_ends_the_lease`,
`a_new_plan_carries_the_preview`, `a_preview_too_big_for_its_room_waits_for_the_commit`,
`lane_previews_never_wait_for_a_compile`, `a_note_dragged_away_from_the_playhead_is_ended`,
`a_copied_clip_keeps_its_id_from_preview_to_commit`, `an_audio_lane_gesture_is_not_previewable`,
`an_export_during_a_lane_lease_has_the_accepted_notes`, `an_automation_drag_is_heard_before_it_is_written`.
`crates/server/src/routes_leases_tests.rs`: `lane_leases_through_the_routes`, `a_read_only_window_begins_no_lane_lease`.
`crates/editor/src/clips_tests.rs`: `edit_as_uses_the_ids_it_is_given`, `pinned_gives_each_named_clip_its_hash`.
m2-server 251 and m2-editor 283 pass; clippy `-D warnings` on both clean; the renders 10 of 10. The web is unchanged in A1.

**Red first**

- The server tests against a `lane_begin` that always answers `previewable: false`: 15 of 17 fail, for example
  `a_lane_lease_previews_with_no_revision` at its `previewable` assert, `a_note_dragged_away_from_the_playhead_is_ended`
  at `assert_eq!(d.sounding(), 0, "it ended")`, `lane_leases_through_the_routes` at `previewable`. Guards (pass
  either way, X25): `an_audio_lane_gesture_is_not_previewable`, `a_read_only_window_begins_no_lane_lease`,
  `an_export_during_a_lane_lease_has_the_accepted_notes` (it fails on the stub only at its `lease.unwrap()`).
- The editor tests against `apply_as` minting with a random Tx id and `pinned` returning the gesture as it is:
  `edit_as_uses_the_ids_it_is_given` fails (`left: [("kick/hits", "xa56c32w")]`, `right: [.., "n60v8bc0"]`);
  `pinned_gives_each_named_clip_its_hash` fails (`left: None`, `right: Some("4059356165532d12")`).

**Left**

- Lane leases aren't in the news (`news.leases` stays the knobs'): a window learns of another's drag only by
  `GET /api/graph/leases`.
- The window whose drag was dropped hears why at its next update or keepalive (at most 2 s later), not at once.
- A new plan hears the drag from its first block or the one after (a swap after the activation), not inside its
  prepare (design-8b §9.1's "the actor gives prepare the preview's lane data").
- Between a PLANE-5 preview of an outside lane write and its settle, the dragged lane plays the outside write
  without the drag (the write's few ms on disk); the settle then sends the drag again.

**For gaps.md:** "A dragged or nudged clip is heard only once it lands": the server side of hearing a drag is built
(A2 is the page).

**Contract:** WRITE-22a *Today*: begin, update, commit, cancel for lanes; previews evaluate the gesture the commit
writes, under the commit's Tx id (Q21); an outside write to a dragged lane re-applies the gesture, one to the dragged
clip ends the lease; a new plan carries the preview; audio lanes are applied on release. RENDER-29: lane leases read
`EngineView.accepted`. The planes table's control row names lane leases.

### A2. Lane leases on the page

**In short**

| You do this while the song plays | You hear |
|---|---|
| Drag a clip, or its edge | It plays where it is now, before you let go |
| Drag an automation point, or draw with Draw (B) | The new curve, as you draw |
| Drag, stretch or paint notes in Notes | The notes where they are now |
| Let go | Nothing changes. It is saved once, one Undo step, as before |
| Press Esc before you let go | It goes back. Nothing is saved |

Stopped, a drag works as before. Nothing on screen is new: the sound is the feedback.

**What changed**

- `web/src/lib/laneLease.ts` (new): `begin(graph, gesture, ended)` gives a lease while the song plays
  (`playingNow()`), else null. `update` sends the newest gesture at most once an animation frame, numbered; a
  keepalive every 2 s; `finish(gesture, step)` writes through `send` (one undo step, the same answer and 409 as
  `POST /api/graph/clips`), by the lease's commit, or by `/api/graph/clips` when the server didn't preview it (an
  audio clip); `abort()` cancels. `ended(why)` hears a lease the server ended (a clip changed elsewhere): the caller
  toasts why and puts the drag back.
- `SongTimeline.tsx` (lane A's drag code only): the clip drag (`down`: move and either edge) begins a lease at its
  4 px threshold and updates it on each move; `drop` writes through `finish` when one lives. `gestureOf(drag)` is
  what both send. The point drag and Draw's paint (`pointsFrom`) begin one at the first change; `sendPoints` takes
  it. Each drag adds a capture-phase Esc listener on the window while it lives (X34): Esc cancels the drag and its
  lease, and stops the event, so the page's deselect doesn't run.
- `NoteView.tsx` (A's imports after its last import, the drag, paint and `send`/`commit` path): a note move,
  resize or paint calls `hear(g, shape)` on each change, which begins a lease at the first, sending
  `{op: "notes", clip, notes, length}`. Let go, `commit` hands the lease to `send`, which writes through
  `afterLaneWrites(…, lease.finish(…))` instead of `writeLane`, so it queues behind a roll write still on its way
  (X17). No lease while a write of the roll is on its way or shown before the graph has it, or on a lane with a
  clip without its own id. Esc (capture phase), a lost pointer and the view unmounting cancel it. Menu tools, keys,
  typed edits and the velocity strip are as before.
- `web/src/lib/graph.ts`: `afterLaneWrites(graph, f)`, X17's function (P0 didn't run, so lane A added it): `f`
  chained on the lane writes' queue and counted in `pending`, its revision in `laneWriting`'s.
- `web/src/lib/clips.ts`: `send` takes `via` (another route that answers as `/api/graph/clips` does); the
  `Gesture` type gains `notes`.
- `web/src/lib/clock.ts`: `playingNow()`.
- `e2e/s8-leases.mjs` (new, kind `fix`, 39 checks), its row in `run.mjs` and `README.md` after `s8-automation`.

**Tests.** `s8-leases` 39 of 39: a Bass clip dragged a bar while playing (one begin, 9 updates, one commit; the
lease listed mid-drag; no revision or file change mid-drag; one revision after, the id kept, "Undo moving a Bass
clip"); Esc mid-drag (a cancel, no write, drawn where it was, still selected); a clip changed elsewhere mid-drag
(the toast "Bass changed elsewhere: your drag was dropped.", nothing written on release); a second window closing
mid-drag (its lease gone in a few ms); stopped (no begin, one `clips` write); a Cutoff point dragged while playing
(begin, updates, commit, "Undo moving a Cutoff point"); a note dragged in Notes while playing (a commit with
`op: "notes"`, no `/api/graph/lane`, "Undo Bass notes"); the taste lint of the timeline mid-drag; a PC page's Esc.
The suites it moves, all green: `taste` 39, `s8-clips` 94, `s8-automation` 45, `pl-notes` 102, `notes` 52,
`gestures` 30, `g11-play` 24. The web build type-checks. The server built at A1's commit; the page this commit's.

**Red first.** `s8-leases` on the page from before (`DIST=scratch/a-dist-before`, the same server): 20 FAIL, 19
pass, for example:
```
FAIL  mid-drag the server lists a lane lease on Bass's notes: []
FAIL  one begin, an update a frame (0 begin, 0 updates)
FAIL  Esc mid-drag: a cancel, no write (clips)
FAIL  a clip changed elsewhere mid-drag: the window says why ()
```

**Left**

- A nudge (← →) and the menu's tools are still heard once they land (no drag to hear).
- The velocity strip isn't leased: a velocity is heard at the next note's On anyway.
- A drag that begins while a write is on its way, or on a lane with an anonymous clip, is heard on release, as
  before.

**For gaps.md:** the loose end "A dragged or nudged clip is heard only once it lands": a drag is heard while
playing (clips, edges, automation points, notes in Notes); a nudge still on landing.

**Contract:** WRITE-22a *Today*: the page's drags while playing lease (taste: nothing on screen is new); Esc during
a drag cancels it and is the drag's alone (X34).

### A3. The tempo lease (with Q23's fallback; A4 not built, X3)

**In short**

| You do this | You hear and see |
|---|---|
| Drag the tempo up or down, playing or not | The song plays at the new tempo as you drag. The number turns blue while you hold it. The time follows |
| Let go | It is saved once. One Undo step: "Undo Tempo" |
| Press Esc before you let go | The tempo goes back. Nothing is saved |
| A song with an audio clip (like Resample) | The new tempo is heard when you let go, as before. Its tooltip says so |
| Someone else changes the tempo while you drag | Your drag stops, and nothing is saved |

**What changed**

- `core/song/src/data.rs`: `DataSet::with_arrangement(arr)`: the same lanes, states and assets (each shared), with
  `arr` and its tempo map.
- `crates/server/src/graph/tempo_lease.rs` (new): `tempo_begin`, `tempo_update`, `tempo_keepalive`, `tempo_cancel`,
  `tempo_commit`, `tempo_leases`; `tempo_expire` and `tempo_gone` (called by `expire` and `client_gone`); for the
  actor, `wanted` and `heard`. One drag at a time: the window that holds it may begin again; another is refused
  ("The tempo is being dragged in another window.").
  - An update keeps the newest value and queues one actor job unless one is queued (Q22). The job settles; when
    its plan was sent and a newer value waits, it queues itself again, behind whatever else was asked. So one
    prepare is in flight, the newest value next, and a drag held during a compile is heard after it.
  - The commit marks the lease closing and runs `Graph::tempo` (8b.2's builder): one Tx writing
    `data/arrangement.json`; its settle plays the accepted arrangement. 409 when the accepted arrangement isn't the
    one the drag began on (X24): "The tempo changed elsewhere: your drag was dropped."
  - Cancel, the TTL (10 s) and the window closing end it and queue the job, whose settle goes back.
- `graph/actor.rs` (lane A's blocks): `Plays.tempo: Option<Arc<Arrangement>>` (the arrangement a drag plays);
  `way()` takes the drag's arrangement: it isn't `Same` while what plays has another, and it activates (never
  reaches) while either side has one; the plan is prepared with `with_arrangement` against what plays' own data
  (its leased map when it has one); `build()` takes the data to play. `settle` reads `tempo_lease::wanted` first
  (none while a lane of what plays is audio: Q23's fallback), sets `Timing.tempo` to the map that plays, and marks the
  value heard after a plan is sent. `settle_as(install, view)`: the tempo job's settle doesn't publish the view again
  when it shows the accepted revision. `Actor::tempo_preview(token)` is the job.
- `graph/preview.rs`: no PLANE-5 preview while what plays is a drag's tempo map (its lane swaps would be timed in the
  accepted map); the settle activates instead. One line in `lend`.
- `graph.rs`: `ControlPlane.tempo` at its end; `mod tempo_lease;`; `pub use tempo_lease::TempoCommitted`; one line each
  in `end_all`, `expire` and `client_gone`.
- `routes_leases.rs`: `POST /api/graph/tempo/lease/{begin,update,keepalive,commit,cancel}`; begin `{session, at}` →
  `{lease, at, bpm, heard}`; update `{lease, seq, bpm}` → `{seq}`; commit `{lease, bpm}` → `/api/graph/tempo`'s answer,
  409 `changed`. Begin and commit are writes (423 read-only). `routes.rs`: `GET /api/graph/leases` lists it with
  `kind: "tempo"`.
- `web/src/lib/tempoLease.ts` (new): `begin(session, at, ended)`, `update` once a frame, a 2 s keepalive, `finish`
  (the lease's commit, or `/api/graph/tempo` when the lease couldn't begin; records the "Tempo" undo step), `abort`.
- `SongBar.tsx` (`TempoField`'s drag only): a lease from the drag's first step, playing or not; the number `accent`
  while held (taste §3.3); Esc (capture phase) and a lost pointer cancel; the tooltip says "Heard as you drag", or
  "Heard when you let go" in a song with an audio lane.
- `e2e/s8-leases.mjs`: the tempo half (16 checks, 55 in all); `e2e/s8-songbar.mjs`: two checks follow (the tooltip's
  words; a drag is written by the lease's commit, not `/api/graph/tempo`).

**Tests** (`graph/tempo_lease_tests.rs`, a driven player): `the_tempo_is_heard_at_the_first_block_after_its_prepare`
(the status's 240 BPM; an E2 at beat 4 struck at frame 48000, not 96000), `one_tempo_prepare_in_flight` (50 updates
while the session is busy: one activation, carrying the 50th value), `the_tempo_prepare_is_measured`,
`a_stale_tempo_prepare_is_done_again` (an edit's new plan plays at the dragged tempo; the drag goes on),
`a_tempo_commit_is_one_tx`, `a_tempo_cancel_goes_back`, `a_closed_window_ends_its_tempo_lease`,
`an_idle_tempo_lease_expires`, `a_seek_during_a_tempo_lease_lands_on_its_beat` (beat 8 at 240 BPM: frame 96000),
`an_export_during_a_tempo_lease_has_the_accepted_tempo`, `a_tempo_commit_after_another_change_is_refused`,
`a_song_with_audio_hears_the_tempo_on_release`. `routes_leases_tests.rs`: `a_tempo_lease_through_the_routes`, and
the read-only test covers the tempo routes. `core/song`: `with_arrangement_keeps_every_lane`.
m2-server 264, m2-editor 283, m2-song pass; clippy `-D warnings` clean on the three; the renders 10 of 10. `s8-leases` 55 of 55; moved and
green: `s8-songbar` 82, `fx-fixes` 75, `taste` 39, `pc-keys` 37. The web build type-checks. The server built from this commit's tree.

**Measured** (design-8b §9.5, information only, Linux under other tracks' load): a tempo preview's prepare, p50 of
21, on the test's one-track song: 0.08 ms.

**Seen, not ours:** in full `m2-server` runs on this busy machine (load average about 17),
`audition_never_waits_for_a_compile` (its compile span not found among the last 16) and
`loading_is_published_while_a_plugin_is_created` (known: 8b's and polish's notes) each failed once; each passes alone
(3 and 6 runs), and the next full run passed whole.

**Red first**

- The server tests with `tempo_update` a stub that keeps nothing and queues nothing: 11 of 13 fail
  (`timed out waiting until the value is heard`), and `a_tempo_lease_through_the_routes` (the status never says 130).
  Guards (pass either way): `a_song_with_audio_hears_the_tempo_on_release`, `an_idle_tempo_lease_expires`.
- `with_arrangement` a stub returning the data as it is: `left: Arrangement { tempo: [.. bpm: 120.0 ..] }`,
  `right: .. bpm: 150.0 ..`.
- `s8-leases` on the page from before: 29 FAIL, for example
  ```
  FAIL  held, the number is accent: rgb(51, 55, 60)
  FAIL  mid-drag the status's tempo follows: 124
  FAIL  once, by the lease's commit (/api/graph/tempo)
  FAIL  Esc: a cancel, no write (/api/graph/tempo)
  ```

**Left**

- A4, audio that drifts during a tempo drag (X3): a song whose plan reads an audio lane hears the tempo on release,
  as before. To build on 8a's live slice: `AudioPlayer.lease`, `Keep.audio_as_is`, the crossfade back (design-8b
  §9.2).
- Tempo prepares on a control-plane thread (Q22): the actor's job prepares; a drag held during a long compile is
  heard after it.
- A drag of a later tempo change keeps the bars after it where they are in beats (as 8b.2's edit does).
- A lane drag and a tempo drag at once (two windows): the lane's preview is timed in the accepted map until the next
  settle.

**For gaps.md:** "Audio drifts during a tempo drag" left (X3, after 8a). The tempo is heard as it is dragged.

**Contract:** RENDER-27a *Today*: "at the first block after its plan is prepared", one prepare in flight (the actor's
coalescing job, Q22), the leased map in `Timing` and in every plan prepared while it lives; a song with audio hears it on
release (Q23). The planes table's control row names the tempo lease.

## Lane B

## Lane C

## Lane D

## Merge

## End
