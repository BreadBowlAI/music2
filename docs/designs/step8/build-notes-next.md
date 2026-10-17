# Build notes: next

The `next` track (plan: `/home/jerry/m2-runs/s8n/plan.md`). Each part writes under its own heading.

## P0

(The lead's.)

## Lane A

Hear it while you drag (8b.7): lane leases, then the tempo lease. Branch `next-a`.

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

## Lane B

## Lane C

## Lane D

## Merge

## End
