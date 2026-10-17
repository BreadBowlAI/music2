# Next: build notes

What each part of the next track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/s8n/plan.md`. Each part writes only under its own heading.

## P0

P0 never landed as its own commit. Each lane made the seams it needed itself (its module stubs, its `mod` and
`.merge` lines, its suites' own servers), and the merge joined them. See Merge.

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

Play in: the metronome, the keyboard piano, the MIDI keyboard. Branch `next-b`, worktree `wt/nx-b`.

### B0. midir, first (X27)

**In short**

| What | Now |
|---|---|
| MIDI keyboards | `m2-os` can hear every MIDI keyboard on the machine, as they are plugged in and out. Nothing uses it yet (B3 does) |

**What changed**

- `Cargo.toml`: `midir = "=0.11.0"` after `cpal` (Q2, X26). `core/os/Cargo.toml`: `midir` optional, behind a new
  feature `midi = ["dep:midir"]`. Only `m2-server` turns it on (`crates/server/Cargo.toml`), so no other crate builds
  midir or the OS MIDI libraries.
- `core/os/src/midi.rs` (new, `#[cfg(feature = "midi")] pub mod midi;`): `listen(on) -> Listening`. A thread
  `m2-midi` reads the list of inputs every second, opens each new one (its own `MidiInput`), closes gone ones, and
  tells `Plugged(name)`, `Unplugged(name)` and `Bytes(name, &bytes)`. "Midi Through" ports are skipped. System
  messages (clock, sysex, active sensing) are ignored. A port that won't open is tried again after 5 s. Names read
  plainly (`pretty`: ALSA's "Keystation 49:Keystation 49 MIDI 1 24:0" is "Keystation 49"), and two keyboards with one
  name get "… 2". Dropping `Listening` stops the thread and closes every connection; nothing is heard after.
- `core/os/src/{linux,mac}/mod.rs`: `midi_allowed()`. On Linux, `access("/dev/snd/seq")` first, because alsa-lib
  writes a line to stderr each time the sequencer won't open (here: `EACCES`), and the listener tries every 5 s. On a
  Mac, always true.

**`Cargo.lock`** gained only new entries (`git diff Cargo.lock`, 173 lines in, 2 changed):

| New | Why |
|---|---|
| `midir 0.11.0` | the crate |
| `coremidi 0.9.2`, `coremidi-sys 3.2.1` | its macOS side |
| `android-build 0.1.4`, `java-locator 0.1.9`, `jni-min-helper 0.3.4`, `dlopen2 0.9.0`, `process_path 0.1.4` | its Android side (never built) |
| `windows-sys 0.52.0`, `windows-targets 0.52.6`, `windows_{aarch64_gnullvm, aarch64_msvc, i686_gnu, i686_gnullvm, i686_msvc, x86_64_gnu, x86_64_gnullvm, x86_64_msvc} 0.52.6` | its Windows side (never built) |

The two changed lines only add a version to a name that now has two versions (`"dlopen2"` → `"dlopen2 0.8.2"` in
`tao`, `"windows_i686_gnullvm"` → `"… 0.53.1"` in `windows-targets 0.53.5`). No existing entry's version moved. It
reuses `alsa 0.11.0`, `alsa-sys`, `libc`, `log`, `bitflags 2`, `objc2`, `block2`, `core-foundation`.

**Tests** (`core/os`, `--features midi`): `listening_never_errors` (here MIDI won't open: no keyboards, no stderr
line; drop stops the thread in under 2 s; nothing heard after), `through_ports_are_skipped`, `names_read_plainly`.
All 41 `m2-os` tests pass with the feature; clippy with and without it; the Mac lint with it (it built
`coremidi-sys` with the stub C compiler).

**Red first:** none; a new module (its tests are guards).

**For gaps.md:** none yet (B3).

**Contract:** PLAT-2's *Today*: `m2_os::midi` (midir; one file, no OS folder; `midi_allowed` per OS).

### B1. The metronome

**In short**

| What | Now |
|---|---|
| The button | A Metronome button sits right after Loop in the top bar. Click it, or press K, or use Transport ▸ Metronome |
| What you hear | While the song plays: a click on each beat, a higher, louder one on each bar's first beat. In 6/8 it clicks the eighths |
| Kept | Each song keeps its own setting. A new song starts with it off |
| Exports | Never have the click |
| Small windows | In the Mac app at its smallest (1100 px), the song's name now goes from the top bar: there is no room for it beside the new button. Taste §3.8 drops it after the time |

**What changed**

- `core/runtime/src/click.rs` (new): `Click`, plain fields. `mix(frames, runs, renderer)` finds the grid points
  (every `4 / unit` quarter notes of the meter in effect) whose frame falls in a moving run, `[at, at + n)`, and
  starts a blip there: a sine, 30 ms, decaying (time constant 6 ms) with a linear fade to 0 at its end; 1,500 Hz at
  −12 dBFS on a bar's first beat (`bar_at(g).1 == 0`), else 1,000 Hz at −16 dBFS; the same in both channels. Off and
  not ringing it returns at once, so the output keeps every bit. Its math is `libm`'s (PLAT-4: `host_math` checks
  `m2-runtime`'s files). Its tests are `click/tests.rs`, a child of `click` (X4: `tests.rs` untouched; a
  `#[path]` would hide the file from `host_math`'s module walk, so it is the plain child path).
- `core/runtime/src/engine.rs`: `Engine.click`, `Engine::set_click`, and one line in `render` after
  `let runs = *cur.last_runs();`. It mixes before the hold's fade, and a held engine renders nothing, so an
  export's hold silences it.
- `core/runtime/src/lib.rs`: `pub mod click;`; `Renderer::arrangement()` (the meters), after `tempo()`.
- `core/play/src/lib.rs`: `Shared.click` (after `running`), `Player::set_click` and `Player::click` (after
  `held`), and in `Callback::render`: `shared` in the destructure and `l.engine.set_click(…)` before `l.fill(…)`.
  So the flag outlives a project: whatever plays next clicks as the player was told.
- Server (X9: no new route): `view_file::KEYS` gains `metronome` (7 keys). `set_view` tells the player when
  `metronome` is among the changes; `view_file::metronome(local)` reads it (off unless `true`). `actor.rs`, two
  one-line blocks: in `Actor::open`, after `local` is read, the player gets the song's; in `settle`'s `Way::Load`
  arm (a new player after a device change), the player gets it from the control plane's `local` (so a read-only
  window's in-memory setting holds).
- Web: `lib/metronome.ts` (new): `useMetronome`, `setMetronome` (`setLocal("metronome", true | null)` and a flush
  at once, not after the usual 300 ms), `toggleMetronome`. `TopBar.tsx`: the 28 px toggle after Loop
  (`Metronome` icon, `accent-soft` when on, tooltip "Metronome: a click on each beat (K)", X36). `App.tsx`:
  `k: "metronome"` in `key()` and `case "metronome"`. `Shortcuts.tsx`: Transport "Metronome on / off: K".
- Desktop `menu.rs`: `ITEMS` `metronome` ("Metronome", no accelerator: K is the page's) after `panic`; Transport:
  Loop, Loop Selection, Metronome, Follow Playhead (X14); the old assertion now reads Metronome then Follow; a test
  `the_metronome_has_its_row_in_transport`.
- Suites moved: `s8-songbar` and `fx-fixes` checked the song's name at 1100 px in the Mac app. With the 32 px, the
  name has 22 px of room there, under the 56 it needs, so it goes (§3.8's order). Each now checks it is gone at
  that one size (same counts), and still clears the tempo and Undo at 1180 and 1280. Their README rows say so.

**Tests**

| Test | Where | Result |
|---|---|---|
| `the_click_sounds_on_each_beat_louder_on_the_bar`, `the_click_follows_the_meter_and_a_tempo_change`, `the_click_lands_on_a_loop_wrap`, `no_click_while_stopped_or_held`, `the_click_off_changes_no_bit` | `core/runtime` (`click::tests`) | ok |
| `the_click_never_allocates` | `core/play/tests/no_alloc_click.rs` (new file, X5) | ok |
| `the_metronome_is_kept_per_song` (set, other song, reopen, a new player after a device change, a read-only window's in memory) | `crates/server` (`view_file_tests.rs`) | ok |
| `an_export_never_has_the_click` (a guard, X25) | `crates/server` | ok |
| `the_metronome_has_its_row_in_transport` | `music2-desktop` | ok |
| `nx-click` (new, after `s8-songbar`) | 20 checks, with a capture server of its own (`M2_AUDIO_CAPTURE`, `M2_MIDI=none`, an empty song) | ok |
| `s8-songbar` 82, `fx-fixes` 75, `pc-keys` 37, `taste` 39 | moves | ok |

All tests of `m2-runtime`, `m2-play`, `m2-os`, `m2-server`, `music2-desktop`; their clippy; the Mac lint of the
five; the web build; the renders 10 of 10.

**Red first**

- `click::tests` with `set_click` a no-op: all five fail, e.g.
  `left: [] right: [0, 24000, 48000, 72000, 96000, 120000, 144000, 168000, 192000]` ("a click on each beat, at its
  frame"), `left: [] right: [0]` ("Play clicks the beat it starts on"). (The bit test fails at "on, it clicks".)
- `the_click_never_allocates` with the callback's `set_click` line out: "the click sounded over a silent song".
- `the_metronome_is_kept_per_song` with `metronome()` answering false: fails at "on at once";
  `a_read_only_window_keeps_view_settings_in_memory` at "a read-only window's metronome clicks too".
- `nx-click` on `dist-before` and the 70f9b61 server:
  `FAIL  the top bar has a Metronome toggle, off for a song that never had it on (null)`,
  `FAIL  it sits right after Loop, 28 px, 4 px from it (… metronome undefined …)`.
  The new page on the 70f9b61 server (the view route refuses `metronome`):
  `FAIL  the song's view.json keeps it: {}`,
  `FAIL  on and playing, a click on each beat, exactly 0.5 s apart, each bar's first the louder ()`.

**Guards** (X25, not red first): `an_export_never_has_the_click`, `the_click_off_changes_no_bit`'s "never on"
half.

**What's left**

- The metronome's level and sound, a count-in (with recording), a click on a cue output (Settings, later).
- An `m2` command for it (Q35, gaps #37).

**For gaps.md:** #19 done: the metronome (count-in left, with recording; its level and sound left).

**Contract:** RENDER-28 (the click: plain fields, no allocation, `no_alloc_click.rs`); RENDER-1 and RENDER-3 (the
click is the live engine's alone: exports never make an `Engine`); FMT-20 (`view.json`'s `metronome`, a view
setting); taste §3.3 (the metronome exists, x 176) and §3.8 (the name goes at the Mac app's 1100 px).

**For 8a's merge** (each a block of its own):

- `core/runtime/src/click.rs`, `core/runtime/src/click/tests.rs`: new files.
- `core/runtime/src/lib.rs`: `pub mod click;` after `pub mod cells;`; `Renderer::arrangement()`, right after
  `Renderer::tempo()`.
- `core/runtime/src/engine.rs`: `use crate::click::Click;` above the `use crate::{…}` line; `Engine.click` at the
  struct's end and `click: Click::default()` at `new`'s end; `pub fn set_click` right after `new`; in `render`, one
  line after `let runs = *cur.last_runs();`: `self.click.mix(&mut frames, &runs, cur);`.
- `core/play/src/lib.rs`: `Shared.click: AtomicBool` after `running`; `Player::set_click` and `Player::click` after
  `Player::held`; in `Callback::render`, `shared` added to the destructure and
  `l.engine.set_click(shared.click.load(Ordering::Relaxed));` before `l.fill(…)`.
- `core/play/tests/no_alloc_click.rs`: new file.

### B2. The keyboard piano

**In short**

| What | Now |
|---|---|
| Turn it on | ⌘K (Ctrl+K on Linux), from anywhere, even while typing. Or View ▸ Keyboard Piano, or the keyboard button in Notes, beside Hear notes |
| Play | A S D F G H J K L ; ' are the white keys, W E T Y U O P the black ones. A is middle C (C4) at first |
| Octave | Z goes down an octave, X goes up |
| What plays | The selected track. A card at the timeline's lower right shows the keys, their notes, the octave, and "Plays Bass" |
| Its keys | While it is on, those 20 keys only play notes. Their shortcuts (A, S, E, X, Z, K) wait until it is off. Space, C, M and the rest work as always |
| Typing | Typing in a field or the code editor never plays notes. A dialog takes the keys too |
| Off | ⌘K again, the card's ×, or the button. It is always off when the app starts |

**What changed** (web only, and one menu row)

- `lib/playTarget.ts` (new): `playTarget(graph, selected, clips)`: the selected track's notes lane, the lane of
  the clip Notes shows when it is that track's, else the first; null for the Song row, nothing selected, or no
  notes lane (Q4). `usePlayTarget`, `currentTarget`. B3 shares it.
- `lib/piano.ts` (new): `usePiano` `{on, octave, held}`; `NOTE_KEYS` by `e.code` (Q7); `setPiano`,
  `togglePiano`, `letGo`. While on, capture-phase `keydown` and `keyup` on `window`: a piano key with no ⌘ ⌃ ⌥,
  outside a field, the code editor (`.monaco-editor`) and an open `[aria-modal="true"]` is
  `preventDefault` + `stopImmediatePropagation` (so no shortcut runs) and `strike(…, {always: true})` on the
  target; its key up releases that note. A repeat strikes nothing. Z and X clamp the octave to C1–C7. `blur`,
  `visibilitychange`, and a Meta key down or up let every note go (X16). With no target, a key is still the
  piano's and plays nothing. While on it holds an `attach()` of audition's, so the server's answers and a closed
  socket are heard.
- ⌘K (X15): a capture-phase listener of `piano.ts`'s own, in a browser only, before anything else sees it, so
  it works from fields and from Monaco (whose ⌘K chords give way). In the desktop app the menu's
  `CmdOrCtrl+K` sends `piano`, as every ⌘ chord does there. So `App.tsx`'s `chord()` has no ⌘K line: the
  capture listener takes the key first in a browser, and the menu does in the app.
- `lib/audition.ts`: `strike(…, {always})`: the piano plays whatever Notes' Hear notes toggle says.
- `components/KeyboardPiano.tsx` (new), mounted after `<Toasts />` (X30): a portal, 16 px in from
  `[data-testid=timeline]`'s lower right (a `ResizeObserver`). `raised`, 1 px `line`, `rounded-md`,
  `shadow-lift`; caps 22×20 px in `text-xs`, the black ones `gray-700` over the white ones (`canvas`,
  `line-strong`); a held cap in the track's `tint`, `bar` and `ink` (X31); the white keys' notes under them
  (`pitchName`, C's with their octave); "Z  C4–F5  X"; "Plays Bass" or "Select a track with notes to play it";
  a × "Keyboard piano off (⌘K)".
- `components/AuditionTools.tsx`: the `Keyboard` toggle after Hear notes (X13). It is the header's other
  toggles' size, 24 px (`sm`), not 28: every Notes tool is 24.
- `App.tsx`: `<KeyboardPiano />`; `case "piano"`. `Shortcuts.tsx` (X32): View "Keyboard piano on / off: ⌘K", and
  a last group "Keyboard piano": "Play notes: A to ' and W to P", "Octave down / up: Z / X".
- Desktop `menu.rs`: `ITEMS` `piano` ("Keyboard Piano", `CmdOrCtrl+K`) after `metronome`; View: after Show
  Browser; a test `the_keyboard_piano_is_command_k_in_view`.

**Tests**

| Test | Where | Result |
|---|---|---|
| `nx-piano` (new, after `s8-audition`) | 39 checks: off at first (A shows automation); ⌘K; the card's words and place; A is 60 at 0.8 on `bass.notes`, lit in Bass's colour, its Off; W 61, K 72 (the metronome left off); X then A 72 (C5–F6); Z Z then A 48; S, E, Z and A run no shortcut; C, M and Space still do; "asdf" in the song's name and in the code editor plays nothing; the ⌥K sheet takes the keys and lists the rows; repeats send one On; blur and ⌘ let go; Keys and the Song row as targets; the lint of the card; ⌘K from a field; the toggle; the ×; a PC page's Ctrl+K | ok |
| `the_keyboard_piano_is_command_k_in_view` | `music2-desktop` | ok |
| `s8-audition` 51, `pc-keys` 37, `taste` 39, `pl-notes` 102, `nx-click` 20 | moves | ok |

The web build; `music2-desktop`'s tests, clippy and Mac lint.

**Red first** (`nx-piano` on `dist-before`):
`FAIL  off at first: the toggle in Notes' tools is off and there is no card (null)`,
`FAIL  ⌘K turns it on: the toggle is lit and the card shows`,
`FAIL  the card says what plays and the octave: null`.

**What's left**

- Velocity keys and a sustain key (Q7: kept small).

**For gaps.md:** #24 (playing live): the computer's keys play the selected track (the keyboard piano).

**Contract:** taste §3.5 (Notes' tools: the keyboard piano toggle after Hear notes), §5 (⌘K is the keyboard
piano, from anywhere; the piano owns its 20 keys while on), the piano card (as built above).

### B3. The MIDI keyboard

**In short**

| What | Now |
|---|---|
| Plug it in | Before or while the app runs. A message says "Keystation 49 is connected. It plays the selected track." |
| The mark | A small keyboard mark shows in the top bar, before the status, while one is plugged in. Its tooltip says "MIDI keyboard: Keystation 49 · plays Bass" |
| Play | Its keys play the selected track, as hard as you press them. The sustain pedal holds notes |
| Unplug it | Its notes stop, the mark goes, and a message says it was unplugged |
| Nothing selected | Keys play nothing. The tooltip says "Select a track with notes to play it" |
| Recording | Not yet: nothing you play is written |

**What changed**

- `m2_os::midi` (B0): `Listening`'s drop no longer joins its own thread (the last owner let go inside an event).
- `crates/server/src/graph/midi.rs` (new): `ControlPlane.midi` (`Midi { keyboards, target }`). `Graph::midi(Event)`:
  `Plugged` joins the list and answers the toast's words; `Unplugged` leaves it, lets go of its notes
  (`audition_gone("midi:<name>")`, one Release) and answers its words; `Bytes` reads the message (`read`):
  `0x9n` with velocity > 0 an On (handle `note + 128 × channel`, pitch the note, velocity `v / 127`), `0x8n` or
  `0x9n` at 0 an Off, held back while the pedal (`0xBn 64`, ≥ 64 down) is down and sent when it lifts; `0xBn 120`
  or `123` let go of all; the rest nothing (Q5). Ons go to the target through `Graph::audition("midi:<name>", …)`;
  Offs need no target, so a note struck before the target changed ends where it sounds. `Graph::midi_target`
  keeps the lane with the song's token: another song opened plays nothing until a window names a lane (X37).
  `Graph::midi_state`: `{devices, target}`, in the state as `midi`.
- `graph/audition.rs`: `Source.keyboard` (a `midi:` source); the watchdog skips it (Q6);
  `expire_auditions_at(now)` for tests.
- `graph.rs`: `pub mod midi;`, `ControlPlane.midi` at the struct's end, `state["midi"]` in `Inner::state`.
- `lib.rs`: `AppState.midi: OnceLock<Listening>` at the struct's end; `spawn_midi` in `start_with` (so the app,
  the headless server and the capture server all listen) unless `M2_MIDI` is `none` or `inject`;
  `AppState::midi(ev)` toasts and sends every window `{"t": "midi", "midi": …}` when a keyboard comes or goes;
  `AppState::midi_changed`; `midi_injects()`.
- `routes_8b.rs`: `POST /api/graph/midi/inject {device, plugged?, bytes?}`, only with `M2_MIDI=inject` (else 404,
  "no such API"); `ws_text` takes `{"t": "midi", "module", "lane"}` (`module: null` clears) and tells every window.
- Web: `lib/midi.ts` (new): sends the target from `playTarget.ts` whenever the selection, the clip Notes shows or
  the song changes, and again on each (re)connect; `useMidi` from the state's `midi` and the `midi` events.
  `components/MidiStatus.tsx` (new): a 28 px `KeyboardMusic` mark in `fg-muted`, its tooltip naming the keyboards
  and the track (or "Select a track with notes to play it" when the lane has gone or none is named).
  `TopBar.tsx`: `<MidiStatus />` before `<Status />`. `App.tsx`: imports `lib/midi`.

**Tests**

| Test | Where | Result |
|---|---|---|
| `a_midi_key_plays_the_selected_track`, `note_off_and_velocity_zero_end_it`, `velocity_is_the_keys` (with the channels and what is ignored), `channels_dont_share_a_key`, `the_sustain_pedal_holds_offs_until_it_lifts` (struck again under the pedal too), `all_notes_off_lets_go` (CC 123 and 120), `unplugging_a_keyboard_lets_go_of_its_notes`, `a_held_midi_key_outlives_the_watchdog`, `no_track_selected_plays_nothing` (and a lane that has gone), `a_new_target_leaves_held_notes_where_they_sound`, `the_target_belongs_to_its_song`, `midi_never_waits_for_a_compile`, `an_export_has_no_midi_note` | `crates/server` (`graph/midi_tests.rs`, new) | ok |
| `nx-midi` (new, after `nx-piano`) | 24 checks on a server of its own with `M2_MIDI=inject` (P0's `env` for suites hadn't landed; this way it needs none), and the 404 on the usual one | ok |
| `s8-audition` 51, `taste` 39, `nx-piano` 39, `nx-click` 20, `pc-keys` 37 | moves | ok |

All `m2-server` (249) and `m2-os` tests; clippy (`m2-os` with and without `midi`); the Mac lint of `m2-os`,
`m2-server` and `music2-desktop`; the web build.

**Red first**

- `a_held_midi_key_outlives_the_watchdog` with `Source.keyboard` always false (today's watchdog): "timed out
  waiting until the quiet window is let go" (both notes went).
- `nx-midi` on the 70f9b61 server: `FAIL  a keyboard plugged in (404)`, `FAIL  a toast says so: []`,
  `FAIL  the top bar's mark names it and what it plays: null`,
  `FAIL  Bass selected, the window tells the server: MIDI plays bass.notes (null)`.

**Guards:** `an_export_has_no_midi_note` (exports never had a live note), `midi_never_waits_for_a_compile` (MIDI
never took the session).

**What's left**

- Recording what you play, a count-in, quantize on input (gaps #24's rest).
- A MIDI input picker; pitch bend, the mod wheel, aftertouch, program change, clock (Q5).
- Notes placed inside a block by their time stamp (`midi_instant`): a note lands at the next block line.
- The Mac by-ear run: whether CoreMIDI lists a keyboard plugged in while the app runs (`m2-midi` polls from a
  thread with no run loop; Not checked in the plan).

**For gaps.md:** #24 (playing live): a MIDI keyboard plays the selected track, with velocity and the pedal;
recording, a count-in and quantize left.

**Contract:** MON-7 (a keyboard is a source, `midi:<name>`, never let go by the watchdog); PLAT-2 (`m2_os::midi`,
only the server's `midi` feature); the state's `midi` and the `midi` event; `M2_MIDI` (`none`, `inject`) and
`POST /api/graph/midi/inject`; taste §3.3 (the MIDI mark before the status pill).

### Lane B: checks at its end, and for the merge

**The desktop app on `:97`** (once, `app-shot.sh` under `flock …/x97.lock`, a scratch HOME, `M2_AUDIO=none`,
the build of `3e65ddd`): Ctrl+K showed the piano card ("Plays Kick", the first track, selected for you); View lists
"Keyboard Piano  Ctrl+K" right after "Show Browser  Ctrl+Alt+B"; Transport lists Loop, Loop Selection,
Metronome, Follow Playhead. Pictures: `~/m2-runs/s8n/scratch/b-shots/run1`, `run2`.

**The renders** after B3 (`3e65ddd`'s `m2`): 10 of 10 match `e2e/renders.sha256`.

**`nx-midi` and P0:** its first check follows `M2_MIDI` in its own environment (run.mjs gives the suite and its
server the same one): 404 without `inject`, 200 with it. So it passes before and after P0's `M2_MIDI=inject`.

**Shared files, lane B's hunks only** (each its own block):

| File | B's lines |
|---|---|
| `Cargo.toml`, `Cargo.lock` | `midir = "=0.11.0"` after `cpal`; the lock's new entries (B0). At a merge of `main`: take `main`'s lock, then `cargo build --offline` adds midir back from the cache |
| `crates/server/Cargo.toml` | `m2-os = { workspace = true, features = ["midi"] }` |
| `core/runtime`, `core/play` | as B1's **For 8a's merge** says |
| `crates/server/src/graph.rs` | `pub mod midi;` after `mod watcher;`; `ControlPlane.midi` at its end; `state["midi"]` after `state["local"]` in `Inner::state` |
| `crates/server/src/graph/actor.rs` | two one-line blocks: in `Actor::open` after `let local = …`, and in `settle`'s `Way::Load` arm before `o.load(…)` |
| `crates/server/src/lib.rs` | `AppState.midi` at the struct's end (and in `new`); an `impl AppState` block (`midi`, `midi_changed`) before `ServeOptions`; `spawn_midi(&state)` after `spawn_meters`; `spawn_midi` and `midi_injects` before `mod tests` |
| `web/src/App.tsx` | imports (`toggleMetronome`, `togglePiano`, `KeyboardPiano`, `./lib/midi`); `k: "metronome"` in `key()`; `<KeyboardPiano />` after `<Toasts />`; `case "metronome"` and `case "piano"` at the switch's end. No ⌘K line in `chord()` (B2): C's ⌘, line is simply the last before `return null;` |
| `web/src/components/TopBar.tsx` | the metronome toggle after Loop (and its import and `useMetronome`); `<MidiStatus />` before `<Status />` |
| `web/src/components/Shortcuts.tsx` | Transport's last row (Metronome); View's last row (Keyboard piano); the last group (Keyboard piano) |
| `apps/music2-desktop/src/menu.rs` | `ITEMS` `metronome`, `piano` after `panic`; Transport's Metronome after Loop Selection; View's Keyboard Piano after Show Browser; the assertion at the old `:501` (Metronome, then Follow); two tests at the end |
| `e2e/run.mjs`, `e2e/README.md` | `nx-piano`, `nx-midi` after `s8-audition`; `nx-click` after `s8-songbar`; the words of the `s8-songbar` and `fx-fixes` rows |
| `e2e/s8-songbar.mjs`, `e2e/fx-fixes.mjs` | the 1100 px Mac-app case only (B1) |

## Lane C

Settings and safety. Branch `next-c`, worktree `wt/nx-c`.

Lane C started before P0 had landed: it made what it needs itself, in its own files and small blocks of the shared
ones. Its suites start servers of their own where they need an environment (`M2_AUDIO=fail`), so they don't need
P0's `env` rows in `run.mjs`.

### C1. A song opens with no sound device

**In short**

| What | Now |
|---|---|
| No sound device when a song opens | The song opens. It plays silently: the playhead moves, nothing sounds |
| The status pill | "No sound output", in the warning colour, with a crossed-out speaker |
| Its popover | "No sound device was found, so the song plays silently. Plug one in: it's used from your next action." |
| A device plugged in | Within about 2 s the song moves to it, where it was, stopped. A toast says "Sound output is back. Press Play to go on." |
| A device that is listed but won't open | Not tried at every action: again only when the default device changes, or after 30 s |

**What changed**

- `core/play/src/outputs.rs` (new): `Choice` (the output and buffer size a player opens; C2 uses it), `Output`,
  `outputs()`, `default_output()`, `output_ready()`. `M2_AUDIO` picks how the process meets the hardware: the
  devices, `none` (the silent player, which lists `M2_AUDIO_OUTPUTS`), or `fail` (no output starts: a test seam).
- `core/play/src/lib.rs`: `Player::start()` is `start_on(&Choice::default())`. `start_on` opens the chosen output
  (C2), or fails in today's words under `M2_AUDIO=fail`. `Player::frames()` and `Player::fell_back()` read two new
  `Shared` atomics. `silent()` is `silent_as("none", None)`.
- `crates/server/src/graph/output.rs` (new): `Outputs`, which starts players (the starter and the probe are
  injectable; unit tests keep `Player::silent()`). When a start fails, the silent player stands in, by its epoch, and
  the log says why once. `Graph::look_for_output()` marks an output to try, and `Graph::no_output()`.
- `crates/server/src/graph.rs`: `Inner::device()` asks `Outputs::start` for a player, and lets a real one take over
  from the stand-in (`Outputs::came_back`), where it was, stopped, with the toast. Not while an export holds the
  player. `Activity` gains `output` (the device's name) and `no_output`. `Inner.output`, `Graph::with_outputs`.
  `let_go` and `retire` are untouched (plan X2).
- `crates/server/src/lib.rs`: the `music2-tell` thread calls `look_for_output()` every 2 s.
- `web/src/lib/settings.ts` (new: the status's `output` and `no_output` types), `Status.tsx`: the `no-output` state,
  after the song's problems and before a dropout.
- `crates/server/src/graph/tests.rs`: one line. A device change's new player now says its name (`output: "none"`).

**Tests**

| Test | Result |
|---|---|
| `core/play` `outputs_put_the_default_first`, `a_chosen_device_that_is_gone_falls_back_to_the_default`, `a_fixed_buffer_is_used_when_in_range` (C2's, built here with `Choice`) | ok |
| `m2-server` `graph/output_tests.rs`: `a_song_opens_without_an_audio_device`, `the_silent_player_says_so_once`, `a_fallback_player_plays_on_its_clock`, `a_device_that_comes_back_is_used_from_the_next_action` | ok, three runs of the whole lib in a row |
| `cargo test -p m2-play -p m2-server`, clippy, the mac lint of both | ok |
| `nx-nodevice` (new, kind `none`, 9 checks) | ok |
| `taste` | 39, ok |

**Red first**

- `a_song_opens_without_an_audio_device` and the rest, with `Outputs::start` passing the start's error on (today's
  open): `panicked at output_tests.rs:46:28: the song opens: no audio output device`.
- With `came_back` never taking over: `panicked at output_tests.rs:119:5: the next action loads the song into the
  new player`; `the_silent_player_says_so_once`: `left: 1, right: 2` (never tried again).
- `core/play`, with `silent_pick` ignoring the choice: `outputs/tests.rs:45`: `left: ("Speakers", false), right:
  ("Headphones", false)`; `outputs/tests.rs:59`: `left: Some(480), right: Some(256)`.
- `nx-nodevice` on the new server with `DIST=` the page before (plan X19: no older server runs with
  `M2_AUDIO=fail`): 3 of 9 passed, then it stopped. `FAIL  the status pill says "No sound output": undefined
  (undefined)`; `FAIL  in warn, with the VolumeX mark: undefined`.

**Left**

- The "Settings…" link in the popover comes with C2.
- The look for an output only runs while a song is open; with none open, the next song's open tries again.

**For gaps.md:** #13 done: a song opens with no sound device, plays silently, the pill says so, and a device plugged
in is used from the next action.

**Contract:** RENDER-28 or the player's section: a start that fails starts the silent player instead, and the status
says `no_output`; the server looks for an output every 2 s while it stands in; a start that failed is tried again
when the default output changes or after 30 s.

**For 8a's merge** (`core/play/src/lib.rs`, each a small block):
- `pub mod outputs;` and its `pub use` line.
- `Shared`: `frames: AtomicU32` and `fell_back: AtomicBool`, after `last_callback_ns` (X6).
- `Callback::fill_in`: one line after `last_callback_ns`'s store: `self.shared.frames.store(…)`.
- `Player::start()` calls `start_on(&Choice::default())`; `start_on` and `start_in` replace `start`'s first lines and
  its device and config lines (`outputs::pick`, `outputs::buffer`, the `fell_back` store). `Choice` folds into
  8a's `Options` (an output name beside `frames`); `M2_AUDIO=fail` and `M2_AUDIO_OUTPUTS` go into
  `Options::from_env()` (plan X1). C never reads `M2_AUDIO_FRAMES` itself: `silent_as` keeps today's line for it.
- `Player::silent()` is `silent_as("none", None)`; `silent_as` is today's `silent` with a name and a buffer size.
- `Player::frames()` and `Player::fell_back()`, after `now_ns()`.
- The unused `HostTrait` import went (the device lookups moved to `outputs.rs`).

### C2. The Settings window

**In short**

| What | Now |
|---|---|
| Open it | ⌘, on a Mac (Ctrl+, elsewhere), music2 ▸ Settings… on a Mac, File ▸ Settings… elsewhere. From the start screen too. The ⌥K sheet lists it under File |
| Sound output | A list: "System default (Speakers)" first, then each device. A chosen device that is gone says "(not connected)" and plays on the default |
| Buffer size | System default, then 64 to 2048 samples, each with its time ("256 samples (5.3 ms)"), only the sizes the device takes. A hint: "Smaller is quicker to respond. Larger is safer if the sound drops out." |
| Now | What plays: "Headphones · 48 kHz · 256 samples (5.3 ms)". If play was on when you changed something: "Playback stopped. Press Play to go on." No toast |
| New songs go in | The folder. In the app: Change… (a folder dialog titled "Choose Where New Songs Go"). In a browser: a field. "Use the default" goes back to `~/Music/music2`. Exports follow the folder |
| Kept | In `~/.music2/settings.json`, for this computer, never in a song, with no undo |
| While an export runs | The output and buffer lists are greyed: "Wait for the export to finish." |
| The status pill | "No sound output" has "Settings…"; "Heavy load" has "Audio settings…" |

**What changed**

- `crates/server/src/settings.rs` (new, shaped like `recent.rs`): `Settings` over a given file, written whole through a
  temporary file; a file that doesn't read is the defaults.
- `crates/server/src/routes_settings.rs` (new, its own router; one `.merge` line in `routes.rs`): `GET /api/settings`
  and `POST /api/settings {output?, songs?}`. Not a project write: no session, works with no song and read-only. An
  output change while an export runs is refused: "Wait for the export to finish."
- `crates/server/src/graph/output.rs`: `Graph::set_output` (the player let go, a new one started at once with the song
  loaded where it was, stopped; whether it was playing), `output_choice`, `output_now` (`Now`).
- `crates/server/src/project.rs`: `songs_dir()` follows `set_songs_dir` (a process-wide `RwLock`), else
  `default_songs_dir()`. `songs_folder()` checks a typed folder: a full path, not inside a song (X38).
- `crates/server/src/lib.rs`: `AppState::keep_settings(file)` reads and applies them (a kept folder that is now inside a
  song is the default); `settings()`; the `mod` lines after `routes_view`.
- `crates/server/examples/headless.rs`, `apps/music2-desktop/src/main.rs`: `keep_settings` beside `keep_recent`.
- `apps/music2-desktop/src/menu.rs`: `settings` ("Settings…", `CmdOrCtrl+Comma`), in the Mac's app menu after About,
  and in File above Quit's line elsewhere. `actions.rs`: `song_folder` takes `{"start", "title"}` JSON (X38).
- Web: `lib/settings.ts` (the API, `useSettings`, `openSettings`, the words for buffers and rates),
  `components/SettingsDialog.tsx` (new), `App.tsx` (the ⌘, line last in `chord()`, `case "settings"` beside
  `shortcuts`, `<SettingsDialog />` after `<Shortcuts />`), `Shortcuts.tsx` (File: Settings), `Status.tsx` (the
  two links).
- The dialog's lists are buttons that open a menu (taste ST-4: no native select). Their menus sit above the dialog
  (`z-[80]!`). ↑ ↓ move in an open menu, Return and Space pick, Esc closes the menu only, Tab closes it.

**Tests**

| Test | Result |
|---|---|
| `m2-server` `settings_are_kept_in_their_file`, `a_bad_settings_file_is_the_defaults` | ok |
| `m2-server` `choosing_an_output_makes_a_new_player_where_it_was` (`graph/output_tests.rs`) | ok |
| `m2-server` `tests/songs_folder.rs`: `new_songs_go_where_settings_say` (its own process, X20) | ok |
| `music2-desktop` `settings_is_command_comma_in_the_app_menu_on_a_mac_and_in_file_elsewhere`, `the_folder_dialog_takes_its_title_from_the_page` | ok |
| `cargo test -p m2-server -p music2-desktop`, clippy and the mac lint of `m2-play`, `m2-server`, `music2-desktop` | ok |
| `nx-settings` (new, kind `none`, 38 checks) | ok |
| `nx-nodevice` (now 11: its Settings… link) | ok |
| `taste` 39, `start` 24, `newsong` 71, `pl-projects` 66, `export` 37, `pc-keys` 37 | ok |

**Red first**

- `settings_are_kept_in_their_file` with `change` never writing: `settings.rs:126`: `left: Choice { device: None,
  frames: None }, right: Choice { device: Some("Headphones"), frames: Some(256) }`. `a_bad_settings_file_is_the_defaults`:
  `settings.rs:151`, `frames: None` against `Some(128)`.
- `choosing_an_output_makes_a_new_player_where_it_was` with `set_output` only keeping the choice:
  `output_tests.rs:160: it was playing`.
- `new_songs_go_where_settings_say` with `keep_settings` not setting the folder: `songs_folder.rs:34`: `left:
  "/home/jerry/Music/music2", right: "/tmp/m2-songs-folder-48080/My Songs"` (it stops before anything is made).
- The desktop tests without the rows: `menu.rs:513`: `left: None, right: Some(("Settings…",
  Some("CmdOrCtrl+Comma")))`; with `folder_ask` ignoring JSON: `actions.rs:266`, the JSON as the folder and the old title.
- `nx-settings` on the new server with `DIST=` the page before: 3 of 38 passed, then it stopped. `FAIL  ⌘, opens the
  Settings window on the start screen`; `FAIL  ⌘, opens it with a song open`; `FAIL  Done has the focus: undefined`.

**Left**

- The Now row says the rate as the app says rates elsewhere ("48 kHz"), not the plan's "48,000 Hz".
- `exporting` greys the lists and the server refuses the change, but no suite runs an export beside Settings (an export of
  first-song is over too fast to catch).
- An input device, settings for every song's view, a theme (plan §10).

**For gaps.md:** #31 done: Settings (⌘,) chooses the sound output and its buffer size, shows what plays, and where new
songs go; kept in `~/.music2/settings.json`. Left: an input device, view settings for every song.

**Contract:** the homes table: `~/.music2/settings.json` (`output.device`, `output.frames`, `songs`), this machine's,
no undo, applied at once. RENDER-31 unchanged: the engine runs at the song's rate whatever the device's. A change of
output or buffer makes a new player where the song was, stopped.

**For 8a's merge:** nothing more in `core/runtime` or `core/play` (C1's blocks are the lane's only ones there).

### C3. Undo says what it undoes

**In short**

| What | Now |
|---|---|
| Undo's and Redo's tooltips | Say what they would take back: "Undo Hats volume (⌘Z)", for every step, made in this window or elsewhere, and after a reload |
| A step an agent or another editor made | A plain name from what it changed: "Undo Bass notes", "Undo the tempo or time signature", "Undo the code in tracks/bass.ts" |
| The app's Edit menu | "Undo Hats volume" and "Redo …" ("Undo" alone with nothing to undo) |
| Undoing a part added or removed | A toast says so: "Undid adding Delay" (as before) |
| Another window | Sees the names too |
| Closing the song | The names go with the undo steps (left: gaps #29's second half) |

**What changed**

- `core/session` (X7): `TxStep` gains `made` (the revision its change made, set by `record_tx`) and `name`
  (`(words, structural)`). `Session::name_step(made, name, structural)` names a step at any depth of undo or redo;
  `Session::step_names()` gives both tops as `StepName {name, structural, file}`; `step()` carries the name to the
  step it pushes on the other stack. Tx, its ops and expectations are unchanged (`the_write_api_is_step_6s` passes).
- `crates/server/src/graph/steps.rs` (new): `named` (`{name, structural, given}` or null) and the plain names by file;
  `Graph::name_step` (an actor job, then a republish, so every read and the menu have it), `Graph::undo_names` (the
  menu's), and `Graph::step`: `undo` and `redo` answer with the name of what they took back, read in the same turn.
- `graph/actor.rs`: two lines in the state after `can_redo`: `undo_name`, `redo_name`. `graph.rs`: `mod steps;`,
  `undo` and `redo` call `step`.
- `routes_settings.rs`: `POST /api/graph/step {session, revision, name, structural}` → `{named, revision, undo_name,
  redo_name}`. Not a write.
- Desktop (X8): the title loop gives `graph.undo_names()` to `menu::set_undo_names`, which sets the items' text when it
  changes; `menu::build` titles them from the last names, so a menu built again keeps them. No `undo_names` action.
- Web: `history.ts` (rewritten): `recorded()` names the step on the server, and says it at once meanwhile; `next(graph,
  redo)` reads `undo_name` and `redo_name` (the window's own answer wins over a state read before the name reached
  the server); `tookBack(answer)`. `TopBar.tsx`: the tooltips' two lines. `GraphCanvas.tsx`'s `graphUndo`: two lines,
  the toast from the answer (`tookBack`) instead of the window's own stacks.

**Tests**

| Test | Result |
|---|---|
| `m2-session` `tops_are_the_steps_undo_and_redo_take`, `two_saves_keep_their_own_names` | ok |
| `m2-server` `graph/steps_tests.rs`: `undo_names_follow_undo_and_redo`, `a_named_step_keeps_its_name_through_undo_redo_undo`, `a_step_made_elsewhere_gets_a_plain_name`, `a_refused_undo_keeps_the_names` | ok |
| `music2-desktop` `the_undo_items_take_a_name` | ok |
| `nx-undo` (new, kind `fix`, 18 checks) | ok |

**Red first**

- `tops_are_the_steps_undo_and_redo_take` with `step()` not carrying the name: `tests.rs:999`: "the undone step is
  redo's, with its name": redo's top `name: None` against `Some("adding Reverb")`.
- `two_saves_keep_their_own_names` with `name_step` naming nothing: `tests.rs:1026`: `assertion failed:
  s.name_step(a, "the first save", false) && …`.
- `graph/steps_tests.rs` with no plain names: `undo_names_follow_undo_and_redo`: `left: ("-", "-"), right: ("the code
  in song.ts", "-")`; `a_step_made_elsewhere_gets_a_plain_name`: `left: "-", right: "Bass notes"`.
- `the_undo_items_take_a_name` with `set_undo_names` doing nothing: `menu.rs:584`: `left: "Undo", right: "Undo Hats
  pan"`.
- `nx-undo` on the new server with `DIST=` the page before: 11 of 18 passed, 7 failed. `FAIL  the server keeps the
  name: {"given":false,"name":"the code in song.ts",…}`; `FAIL  Undo names it plainly: Undo (⌘Z)`; `FAIL  a reload
  keeps the names: Redo (⇧⌘Z)`.

**Moved suites**

| Suite | Result |
|---|---|
| `undo` 10, `s8-tracks` 75, `s8-mix` 80, `taste` 39, `start` 24, `pl-arrange` 57, `pl-notes` 102, `effects` 12 | ok, unchanged |
| `attention` 57 | ok after two of its checks follow the change. They said "Undo names nothing" after an outside edit, since the page couldn't be sure. The server is sure now: an outside edit is no undo step, so Undo still names the step it takes back ("Undo “Move all to Kick (kicks)”", "Undo Xrange hi"). The first check now waits for the page's own read of the new revision instead of the unnamed tooltip |

**Every browser suite** on the lane's build (`lane-suites.sh c … all`, after C3): 50 of 52 ok at first; `live3b` PART
(no `CLAP_PATH`, as always here); `s8-clips` failed twice: `FAIL  as one move gesture, and nothing else sent:
/api/graph/clips …,/api/graph/step {…"name":"moving a Hats clip"…}` and `FAIL  in one gesture`. The page's step
naming is a request beside each write, and `fixlib.mjs` counted every request but a GET as a write. Fixed there, once
for every suite: `/api/graph/step` writes nothing, so it isn't one of `posts`. Then `s8-clips` 94, `s8-automation` 45,
`s8-songbar` 82, `s8-mix` 80, `fixes-graph` 42, `fixes-notes` 37 and `nx-undo` 18: ok.

**Left**

- While the code editor has focus, Edit ▸ Undo undoes typing, though its title names the song's step (X8).
- Names are lost when the song closes (gaps #29's second half).
- A window that didn't make a step sees its name at its next refresh (the namer's own answer isn't broadcast).

**For gaps.md:** #29: names done: every undo step has a name, kept on the server, shown in the tooltips and the Edit
menu, plain words for steps made elsewhere. Left: undo that survives closing the song.

**Contract:** WRITE-21: each undo step keeps the revision it made and an optional name (`Session::name_step`,
`step_names`); the name follows the step through undo and redo. Tx, its ops and expectations are unchanged.

**For 8a's merge:** nothing in `core/runtime` or `core/play`. In `core/session` this touches `session.rs`,
`tx/mod.rs` (`TxStep`'s two fields) and `tx/tests.rs`; 8a's `lock.rs` block is elsewhere.

**Checked in the app** (once, on the invisible screen `:97`, `tools/app-shot.sh` under the `x97.lock` flock): Ctrl+,
opens Settings; File ▸ Settings… Ctrl+, sits above Quit; after an agent's Tx, Edit ▸ "Undo Bass notes". GTK's
`set_text` works with the menu closed; it was not tried while a menu is open.

## Lane D

Effects and small fixes. Branch `next-d`, worktree `wt/nx-d`. Built in the order X29 sets: D4e, D1, D2, the rest of
D4, then D3. All five built; nothing of the lane is left unbuilt.

**For 8a's merge:** none. Lane D touches nothing in `core/runtime`, `core/play`, `core/os`, `crates/gates` or
`apps/`, and `Cargo.lock` is unchanged. In shared files it adds only: `crates/server/src/lib.rs` `mod routes_fx;`
(after `mod routes_view;`), `routes.rs` one `.merge(crate::routes_fx::router(…))` (after `routes_view`'s),
`graph.rs` `mod fx;` (after `mod watcher;`); `e2e/run.mjs` rows `nx-effects` (after `effects`) and `nx-nits` (after
`pl-arrange`), and their `e2e/README.md` rows.

### D4e. A first options object on its own line (Q31)

**In short**

| What | Before | Now |
|---|---|---|
| Moving the master fader in a song written over lines | `}), { db: -2.5 },` on the mixer's last line | `{ db: -2.5 },` on a line of its own, lined up with the mixer |

**What changed**

- `crates/editor/src/lens.rs`, `Insertion::plan`, the arm that writes a new options object after the last argument:
  when the call's first argument starts on a new line after its `(`, the object goes on a line of its own at the last
  argument's indent. After the last argument's comma when it has one (`\n    { db: -2.5 },`), else with a comma
  before it (`,\n    { db: -2.5 }`). A line comment after that comma stays after the new object. A call on one
  line, or one whose first argument starts on the `(`'s line (`master(mixer({\n …\n }))`), is written as before.
- The empty call written over lines (plan D4e) is not built: no std component takes only options, so nothing reaches
  it.
- History's words don't change: `fold` drops the line breaks and the comma before `)`, so it still says
  `master(…, { db: -2.5 })`.

**Tests**

- `crates/editor`: `a_first_options_object_goes_on_its_own_line` (five calls: over lines with and without a trailing
  comma, a tab indent with a line comment, one line, first argument on the `(`'s line; each undone back to the
  bytes). `the_master_fader_writes_master` (`mix_tests.rs`) now expects the object's own line. 282 of 282
  `m2-editor` tests, clippy clean.
- `s8-mix` reads the code it writes: the master's `{ db: … },` must be on its own line, between the mixer's `}),`
  and `),`. 80 of 80. `taste` 39 of 39.

**Red first**

- The test without the `lens.rs` change:
  `left: "…master(\n    mixer({ a: osc.sine(220) }), { ceiling: -3 },\n  ),…"` against
  `right: "…master(\n    mixer({ a: osc.sine(220) }),\n    { ceiling: -3 },\n  ),…"`.
- `s8-mix` on the server from before (`bin/d-70f9b61`):
  `FAIL  the master's fader writes master(…, { db: … }) on its own line: "…    }), { db: -4.8 },\n  ),\n);\n"`.

**Left:** the empty call written over lines (nothing reaches it).

**For gaps.md:** none (a fix).

**Contract:** an options object the app writes into a call written over lines gets a line of its own.

### D1. Turn an effect off and on (Q24, Q25, X10, X11)

**In short**

| What | Now |
|---|---|
| An effect card | Has a ⋯ button (on hover). Its menu says **Turn off** or **Turn on** |
| Turn off | The sound goes on without the effect at once. The card stays where it was, dimmed, and says "Reverb · Off". Its knobs stay, to set |
| In the code | `reverb(x, { size: 0.6, bypass: true })`. Turn on takes `bypass: true` out again |
| Undo | One step: "Undo turning Reverb off" |

**What changed**

- `core/script/sdk/core.ts`: a call of a component whose first argument is an audio input and whose output is audio
  (and that declares no `bypass` of its own) takes `bypass: true` in its options. The body runs as always, the frame
  records `bypass`, `produced` keeps the call's own output (the editor finds the call by it), and the call returns
  its audio input. Nothing reads the effect's nodes, so they are not in `Flat.order` and the engine never makes them
  (`core/ir/src/check.rs` keeps only what the output reaches; no warning). **The engine doesn't change.** A call of
  anything else refuses `bypass` in words ("can't be bypassed: only a call that takes audio first and gives audio
  can be"); `bypass: "yes"` is refused too. Types (X10a): `CallOptions` gains `bypass?: boolean` for such a component,
  and its call type always has the options form (`NeedsOptions`), so the code editor marks nothing.
- `core/ir/src/lib.rs`: `Frame.bypass: bool`, written only when true (ID-11): no IR of today moves.
- `crates/editor/src/effect.rs`: `bypass(s, at, call, on)`, `is_bypassed`, `bypass_refused`. On: `bypass: true` after
  the options' last property, into empty braces, `false` made `true`, or a new options object by `Insertion::plan`
  (D4e's rule for a call over lines). Off: the property out with its comma, and the object when it was alone. Words
  when it can't: the call spreads its arguments, its options are not written in the call, a comment beside it, "It has
  its own Bypass knob." The planner asserts: with the effect's own nodes taken out of the graph where it is off, that
  graph is exactly the one where it is on with the effect removed (`wraps`, as remove asserts), and the frame says so.
  Its expectation is `Seen` with a probe first (R26), as rekey's is: the call around the effect reads the effect's
  input while the effect's nodes stay unchanged, which no `Confined` range can say. One `TextEdit`, inside the call.
- `crates/editor/src/view.rs`: a node carries `bypassed` and `menu` (`EffectMenu { bypass }`: why the row can't run).
  For an effect turned off, what the call around it reads (the reader whose call holds the effect's, or the module's
  export) is drawn from the effect's card with `PortEdge.bypassed`, so the Device view keeps the card in the chain
  (X10c). `controls.rs`: `options_of` is `pub(crate)`.
- Server: `POST /api/graph/effect/bypass {session, revision, node, on}` (`routes_fx.rs`, its own `WRITES_FX` and 423
  layer; `graph/fx.rs`). Lines in shared files: `lib.rs` `mod routes_fx;`, `routes.rs` one `.merge`, `graph.rs`
  `mod fx;`.
- Web: `web/src/lib/effects.ts` (new; its types added to graph.ts's by `declare module`), `NodeGraph.tsx`: the card's
  ⋯ (`Ellipsis`, shown on hover and focus) opens a `Popover` of `MenuRow`s (X11; no Power button); a row that can't
  run is greyed with the words in its tooltip. An off card: title "Reverb · Off" in `fg-muted`, body at 50%; the
  line out of it dashed in `line-strong`. No toast (a direct edit, ST-5); Undo names it.

**Tests**

- `core/script`: `bypass_returns_the_audio_input` (the song reads the saw; the frame says `bypass`; the call's own
  output is still the delay's; `bypass: false` is on; no `bypass` in an IR that has none on), `bypass_keeps_the_frame_
  and_its_knobs` (the same constants and frames, three call forms), `bypass_is_refused_on_an_instrument` (follow,
  noise, square, mix, a notes-in instrument; a string; a component's own `bypass` stays its knob). 59 of 59.
- SDK types (`web`'s typecheck, `core/script/sdk/check`): `calls.ts` and `std.ts` gain the forms (`gain(x, -3,
  { bypass: true })`, `duck(x, l, { depth, bypass })`, `master(x, { bypass: true })`, …) and four refusals.
- `crates/editor`: `bypass_writes_bypass_true_into_the_options`, `bypass_into_a_call_with_no_options` (five forms),
  `bypass_off_takes_it_out_and_its_braces` (five), `bypass_then_unbypass_is_the_same_code` (six layouts),
  `a_bypassed_effect_runs_nothing` (to the bit as removed), `bypass_refuses_a_call_that_spreads_its_arguments` (and on
  when on, and its own Bypass knob, in the view's menu too), `a_bypassed_effect_keeps_its_place_in_the_chain`. 289 of
  289. Clippy clean on `m2-ir`, `m2-script`, `m2-editor`, `m2-server`; `m2-server` 234 of 234.
- Suite `nx-effects` (new, `fix`, 25): the menu, its tooltip and lint; Turn off writes only `bypass: true`; the card
  is off, "Reverb · Off" in `fg-muted`, body 0.5, in the same place, the line out dashed; no toast; "Undo turning
  Reverb off"; lint of the off card; the menu says Turn on; the code editor marks nothing; ⌘Z; Turn on restores the
  options; a PC page's menu. Moves: `effects` 12, `s6-builders` 56, `s5-knobs` 120, `graph` 19, `taste` 39: all pass.
- The renders: 10 of 10.

**Red first**

- `core/script` without the SDK change: `delay(): no option "bypass" (options: time, feedback, mix)`; `assertion
  failed: refused(…follow(…, { bypass: true })…).contains("follow(): can't be bypassed")`.
- SDK types without it: `calls.ts(129,14): error TS2353: Object literal may only specify known properties, and
  'bypass' does not exist in type '{ time?: …; feedback?: …; mix?: … }'`; `calls.ts(134,15): error TS2554: Expected
  1-2 arguments, but got 3.`
- `crates/editor` with `bypass` a stub: 6 of 7 fail (`called Result::unwrap() on an Err value: Rejected("stub")`).
- `nx-effects` on the page and server from before: `FAIL  the Reverb card's ⋯ menu: `, `FAIL  Turn off writes
  bypass: true into the call, and nothing else`, `FAIL  the card is off`.

**Left:** a turned-off effect bound to a name read elsewhere (`const wet = reverb(x, { bypass: true })` read later)
draws its card as a branch, not in the chain. `m2` has no bypass command (Q35). Delete and Edit in code are not in
the ⋯ menu (the card's own buttons stay).

**For gaps.md:** #21: an effect can be turned off and on (bypass), kept in its code.

**Contract:** `bypass: true` in any audio-to-audio component call's options gives its audio input on; its frame
records `bypass` (IR, written only when true); the engine never runs it. Route `POST /api/graph/effect/bypass`.

### D2. Move an effect before or after another (Q26, X11)

**In short**

| What | Now |
|---|---|
| The ⋯ menu | Also has **Move left** and **Move right** |
| Move left on Delay, after Reverb | Delay goes before Reverb. Each keeps its own settings. The cards swap |
| When it can't | The row is grey. Its tooltip says why: "Reverb and Delay aren't next to each other in the code, so they can't swap here. Change their order in code.", or "Delay is the last effect here." |
| Undo | One step: "Undo moving Delay before Reverb" |

**What changed**

- `crates/editor/src/effect.rs`: `reorder(s, at, call, earlier)`. Only two effects written one directly inside the
  other: `outer(inner(x, I), O)` becomes `inner(outer(x, O), I)`. Two edits: the heads (`outer(` and `inner(` swapped,
  the gaps after each `(` kept where they were) and the tails (`, O)` and `, I)` swapped). The sound they work on (`x`)
  isn't touched, so its nodes keep their names. Confined to the outer call. The planner asserts that the outer call
  reads the inner one's output in each graph (or the inner one is turned off), and that with both effects taken out
  the two graphs are the same. `wraps` is now built on `reads_of` and `unwrapped` (the graph without a call, one per
  input it could stand for), which the assertion uses twice.
- Words (Q26): effects next to each other in the sound but not in the code; "Delay is the first effect here." and
  "… the last effect here." for a row with no effect on that side; calls that spread their arguments.
- `view.rs`: `EffectMenu.left`, `.right`: `MoveRow { with, refused }` (the effect it swaps with, or why not).
- Server: `POST /api/graph/effect/move {session, revision, node, earlier}` (`routes_fx.rs`, in `WRITES_FX`;
  `graph/fx.rs` `move_effect`).
- Web: `moveEffect` in `effects.ts`; the menu's two rows (`ArrowLeft`, `ArrowRight`), greyed with the words in the
  tooltip, "Move Delay before Reverb" when they run. No toast: the cards swap on screen. A card kept across an edit
  that now shows another call closes its menu.

**Tests**

- `crates/editor`: `two_nested_effects_swap` (the code, the label `delay(reverb(…))` → `reverb(delay(…))`, the order
  the engine runs them, undo, and Move right on the inner one), `the_options_go_with_their_effect` (positional, none, a
  turned-off one, over lines), `an_effect_not_next_to_another_is_refused_in_words` (and the menu's rows, and the
  first and last), `moving_twice_is_the_same_code`. 293 of 293; clippy clean on `m2-editor`, `m2-server`.
- `nx-effects` gains 14 (39): Delay added after Reverb; its rows and Reverb's, greyed with why; lint of the menu;
  Move left writes the swap; the cards swap; no toast; "Undo moving Delay before Reverb"; ⌘Z; after an outside save
  that binds Reverb to a name, Delay's Move left is greyed with Q26's words. Moves: `effects`, `s6-builders`,
  `graph`, `taste`: all pass.

**Red first**

- `crates/editor` with `reorder` a stub: 4 of 4 fail (`Rejected("stub")`; `assertion failed: words(moved(&mut s,
  "delay", true)).contains(apart)`).
- `nx-effects` on D1's server (`bin/d-d1a`) with this page: `FAIL  Delay's Move left: Move left (off): Move Delay
  before undefined`, `FAIL  Move left: Delay goes before Reverb, each with its own settings`, `FAIL  the cards swap
  places`.

**Left:** a call written over lines keeps its gaps where they were, so its lines read oddly after a swap (it is still
right). Effects not written one inside the other, or in two files, are refused (Q26).

**For gaps.md:** #21: an effect moves before or after the effect written next to it.

**Contract:** Route `POST /api/graph/effect/move`; one Tx that swaps two nested effect calls, each keeping its text.

### D4. Small fixes: a, b, c, f, g (Q28 to Q33, X12)

**In short**

| Fix | Before | Now |
|---|---|---|
| a. The ruler | A marker hid its bar's number. A tempo change touched its bar's number | The ruler is a little taller. Markers on top, bar numbers under them. A change sits on the numbers' line, after its bar's number |
| b. The ruler's end | The last bar number could hide under Follow and zoom | It scrolls clear of them |
| c. A drum lane | Its note menu said "Up a semitone" and offered octaves; black keys behind the drum names | "Up a row", "Down a row", no octaves; every row alike |
| f. Shift + wheel over a fader | On Linux it moved the fader | It scrolls the strips, on every OS |
| g. `s8-mute`'s lint | Its own weaker copy | The shared one (`e2e/lintlib.mjs`) |

**What changed**

- a (X12): `layout.ts`: `RULER_H` 28 → 36, the timeline's; `ROLL_RULER_H = 28` serves NoteView's three uses (its
  import line and the three uses only). `index.css` `--spacing-ruler: 36px`. `dockMax` follows. `SongTimeline.tsx`
  `Ruler`: the markers keep their 14 px band at the top; the bar numbers sit at 17 px (`leading-none`), each in a
  row with the change at its bar line, 6 px after the number; a change off a bar line sits 3 px after its beat on
  the same line. Beat ticks and the loop brace stay at the foot. `Markers.tsx`: its header comment only.
- b (Q29): the timeline's width is the song's room plus `TOOLS_W`; and no bar number is drawn where it would start
  under the tools at the ruler's very end.
- c (Q30): `NoteMenu.tsx` takes `named`: "Up a row" ↑, "Down a row" ↓, no octave rows. `NoteView.tsx`: `Keys` draws
  every row of a named lane alike, a line under each, no black keys; the kit's grid shades no rows (`shaded` is
  empty; `Lanes` reads an empty list as none). `pl-sounds` followed: the kit's rows are unshaded, by a key or as
  black keys.
- f (Q32): `Mix.tsx`, the fader's wheel: with Shift it never steps; deltaY with Shift scrolls the strips
  (`[data-testid=mix-strips]`) itself; deltaX was the browser's already.
- g (Q33): `s8-mute` lints with `lintlib.mjs`; its count stays 152.
- Suites that followed the ruler: `pl-zoom` (the tools are 36 px tall), `s8-songbar` (a bar number's place is its
  row's `left`).

**Tests**

- Suite `nx-nits` (new, `fix`, 26): a marker at bar 5 and its number apart, the point at the number's middle is the
  ruler and no marker; the ruler 36 px, the markers' band 14 px at its top; a tempo change at bar 9 says ♩140, 4 px
  after "9" (boxes; its text 6 px), on one line; one off a bar line 3 px after its beat; lint of the ruler; zoomed out
  to 64 px bars and scrolled to the end, the last number (18) ends left of the tools; a Drum kit's note menu, its
  keys 60 and 61 alike, its grid unshaded, both linted; on a PC page Shift+wheel up and down over Hats' fader leaves
  it and scrolls the strips; a Mac page's sideways the same; lint of the Mix view; the wheel wrote nothing.
- Moves, all pass: `pl-zoom` 62, `pl-arrange` 57, `s8-clips` 94, `s8-automation` 45, `s8-songbar` 82, `pl-notes`
  102, `pl-mix` 47, `s8-mix` 80, `s8-mute` 152, `taste` 39, `fx-fixes` 75, `pl-sounds` 45.
- The narrow strips: Shift+wheel is checked at 380 px wide (as `s8-mix`'s sideways check), where first-song's strips
  overflow; at 1100 px its five strips fit and nothing can scroll.

**Red first**

- `nx-nits` on the page and server from before: 13 of 26 fail, among them `FAIL  nothing of the marker is at the
  middle of "5": marker Chorus`, `FAIL  "9" and "♩140" are at least 4 px apart: -12`, `FAIL  scrolled to the end,
  the last bar number (19) ends left of Follow and zoom: 1412 < 1355`, `FAIL  its note menu moves a row up or down:
  up:Up a semitone↑ | …`, `FAIL  row 61's key looks as row 60's: {… "61":{"black":true,"hit":"black",…}}`, `FAIL  a
  PC page: Shift+wheel up and down leaves Hats' fader: -6 → -6.3`.
- g: the old copy and lintlib on one page with a 14 px text field and a `::before` gradient: `old inline lint: []`,
  `lintlib: ["font-size 14px at <input>","gradient ::before…"]`.

**Left:** nothing of D4 (d is in a).

**For gaps.md:** none (fixes). taste.md §3.2 at the End: the ruler is 36 px, its rows (markers 14 px band; numbers
and changes at 17 px; ticks and loop at the foot).

**Contract:** `RULER_H` 36 (timeline), `ROLL_RULER_H` 28 (roll).

### D3. Pick what Duck listens to (Q27, X28)

**In short**

| What | Now |
|---|---|
| The Duck card | Its side row says **Listens to: Kick ▾** |
| Click it | A list of the song's other tracks, by name. Pick one: the duck now dips when that track plays |
| In the code | `duck(bass.out, kick.env, …)` for a track that has an `env`; `duck(bass.out, follow(hats.out), …)` for any other, and `follow` is imported |
| Undo | One step: "Undo Bass's duck listens to Hats" |
| A Duck in a track's own file | Greyed. Its tooltip: "Bass's Duck is in tracks/bass.ts, which can't hear other tracks. Change it in code." |

**What changed**

- `crates/editor/src/effect.rs`: `sidechain(s, at, call, track)`. The Duck (`std:duck`; the effect browser doesn't
  list it, so it is found by its frame's ComponentId, not `effect_at`) has its side argument (`args[1]`) replaced
  with `<handle>.env` when the track exports `env` (X28), else `follow(<handle>.out)`, importing `follow` from
  `std/dsp` when the file doesn't. `<handle>` is the name the Duck's file default-imports the track by
  (`sites::imports_of`). Refused in words: a Duck in a track's file, a track its file doesn't import, the track it
  listens to already. `Confined` to the argument and the import line (the new `follow` or import node is inside
  them, so the Duck's rewired side is allowed). The planner asserts the Duck's side now comes from that track (an
  import of its export, or a `follow` of one).
- `listens(s, site)` and `Listens { port, ducks, track, choices, refused }`: what the row shows; the choices are
  the tracks the file imports, but the one it ducks, sorted by name. `view.rs`: `ViewNode.listens`, its `port`
  filled with the side input's row.
- Server: `POST /api/graph/effect/sidechain {session, revision, node, track}` (`routes_fx.rs`, in `WRITES_FX`;
  `graph/fx.rs` `listen_to`).
- Web: `listenTo` in `effects.ts`; `NodeGraph.tsx` `ListensRow` in place of the side input's row: "Listens to:",
  the track's name and `ChevronDown` (a 20 px button), its list a `Popover` of `MenuRow`s with a check on the
  current one. No toast (the row says it); Undo names it.

**Tests**

- `crates/editor`: `a_track_with_no_env_gets_follow_of_its_out_and_the_import` (the starter's Bass duck: its row
  says Kick and offers Clap, not Bass; `kick.env` → `follow(clap.out)` and the import; undo puts both back),
  `the_picker_writes_the_tracks_env` (Chords: Hats, then Kick writes `kick.env`; Kick again is refused),
  `a_duck_in_a_track_file_is_refused_in_words` (and its row says so). 296 of 296; clippy clean.
- `nx-effects` gains 8 (47): the row says "Listens to:Kick", its list "Hats,Keys,Kick", lint of the row and list,
  picking Hats writes `follow(hats.out)` and the import and nothing else, the row says Hats, "Undo Bass's duck listens
  to Hats", ⌘Z. Moves, all pass: `effects` 12, `s6-builders` 56, `s5-knobs` 120, `graph` 19, `ports` 46,
  `fixes-graph` 42, `taste` 39, `s8-mix` 80, `pl-ids` 39, `s8-tracks` 75, `pl-tracks` 20. `m2-server` 234 of 234.

**Red first**

- `crates/editor` with `sidechain` a stub: 3 of 3 fail (`Rejected("stub")`).
- `nx-effects` on D2's server with this page: `FAIL  the Duck's side row says what it listens to: null`, `FAIL  picking
  Hats writes follow(hats.out), and imports follow`, `FAIL  Undo: Undo (⌘Z)`.

**Left:** a `follow` import left behind when the Duck goes back to a track's `env` (harmless; unused). A track that
exports another control signal than `env` (an LFO) is still heard through `follow(<its>.out)` (X28). `m2` has no
command for it (Q35).

**For gaps.md:** #21: a Duck's sidechain is picked from a list of the song's tracks.

**Contract:** Route `POST /api/graph/effect/sidechain`; the view's `listens` on a Duck card.

## Merge

**In short**

| What | Now |
|---|---|
| The four lanes | Joined on `next`, as merge commits, smallest first: D, C, B, A |
| What clashed | Only the lines each lane added at the same place. Both sides were kept |
| What the join broke | Listed below, each fixed in its own commit |

**How the shared files were joined.** P0 never landed, so each lane made its own seams and this file. Each lane's
section above is its own branch's, word for word.

| File | Joined |
|---|---|
| `crates/server/src/lib.rs` | `mod routes_fx;` (D), `mod routes_settings; pub mod settings;` (C), `mod routes_leases;` (A), in that order after `mod routes_view;`. `AppState`'s fields: C's `settings`, then B's `midi`, at the struct's end and in `new` |
| `crates/server/src/routes.rs` | The `.merge(…)` lines of D, C and A after `routes_view`'s |
| `crates/server/src/graph.rs` | The `mod` lines of D (`fx`), C (`output`, `steps`), B (`midi`) and A (`lanes`, `tempo_lease`). `ControlPlane`'s fields: B's `midi`, then A's `lanes` and `tempo` |
| `apps/music2-desktop/src/menu.rs` | C's two tests, then B's two; git joined `ITEMS` and the rows itself |
| `web/src/components/TopBar.tsx` | B's `useMetronome()` line, with C's `next(graph, …)` undo names (C changed `next`'s signature) |
| `web/src/App.tsx`, `Shortcuts.tsx`, `SongTimeline.tsx`, `NoteView.tsx`, `e2e/run.mjs`, `e2e/README.md`, `s8-songbar.mjs`, `graph/actor.rs`, `core/play/src/lib.rs` | Joined by git with no clash |
| `Cargo.lock` | Lane B's: midir's entries only (main hasn't moved) |

**Fixes the join needed**

- `core/play/src/outputs_tests.rs` moved to `core/play/src/outputs/tests.rs`, and its `#[path]` line went.
  `m2-runtime`'s `host_math` test walks `core/play/src` by its `mod` lines and can't follow a `#[path]` (lane B met
  the same and put its click tests in `click/tests.rs`). Lane C ran `m2-play`'s tests but not `m2-runtime`'s, so it
  didn't see it. Red: `host_math_goes_through_libm` panicked, `…/play/src/outputs/tests/mod.rs: No such file or
  directory`. After: 1 passed; `m2-play`'s 22 lib tests pass, the three `outputs::tests` among them.
  **For 8a's merge:** only the test file's place and the `mod tests;` line at `outputs.rs`'s end changed.
- `s4-meter` reads the ruler's bar numbers from their rows (`[data-bar]`, placed by the row's left) and the shared
  change from its own element (`[data-mark]`), as lane D did for `s8-songbar`. Lane D's 36 px ruler moved them, and
  `s4-meter` wasn't in D4's list of suites to run. Red, on the joined page: `FAIL bar numbers follow the meter:
  [["1",null],["2",null],…,["7/8 ♩100",null],…]`, then `FAIL the 7/8 and the tempo change share a mark`. After: 8 of 8.
- `pl-arrange` waits up to 2 s for Undo's tooltip to name a step (five checks), as its check after an undo already
  did. It read the tooltip once, as soon as `layout.json` changed, which can be before the window has the new
  revision. In the first joined run, under a load near 17 (two suite batches and the Rust tests at once), it failed
  twice: `FAIL Undo says so: Undo Marker 1 at bar 3 (⌘Z)` (the step before); alone, once. Timed alone four times,
  the tooltip was right at the first look (1 to 3 ms), so nothing in the app is slow: it is the suite's race. A
  guard, not red first. After: 57 of 57.
- `s8-clips` waits to hear of a request before it counts it (`sentBy`, three checks). The browser tells the suite
  of a request on its own channel, and under load that can come after the server has written the file. Red, under
  load (four copies at once, three runs each): 4 of 12 runs gave `FAIL in one gesture: []`; with a 1.5 s wait
  before the look, the same request was there (`at once [], then [{"url":"/api/graph/clips",…}]`). The app sent
  one gesture each time. After: 12 of 12 under the same load. In the first joined run, under heavier load, `⌫
  deletes both` also took over 8 s once; it passed in every later run.
- P0's MIDI line, which no lane could make: `run.mjs` gives every suite's server, and the suite, `M2_MIDI=inject`
  (plan §1.3), and the three suites that start a server of their own without it (`s8-audition`, `live3b`,
  `pl-projects`) give it `M2_MIDI=none` (X35). No suite's server opens a MIDI device, so on a Mac a keyboard
  plugged in never plays into a suite. One line in `e2e/README.md` says so. Before: `nx-midi`'s first check read
  `M2_MIDI unset: 404`; after: `M2_MIDI inject: 200` (it passes both ways by design: a guard). Run after: nx-midi
  24, s8-audition 51, pl-projects 66, taste 39, nx-piano 39, live3b PART 40 of 60 (no `CLAP_PATH` here, as always).
- `s8-tracks` stopped once at its `synth-pad` tab (`dragFrom` undefined) in the first joined run, under that load. It
  passed 5 of 5 runs since, 3 of them under load. Left as it is.

**Tools** (`~/m2-runs/s8n/tools`, not in the repo): `app-shot.sh` takes `scratch/x97.lock` itself (`flock -w 1200`),
so two runs never share `:97`; never wrap it in `flock` too, or it waits on itself. It gives the app `M2_MIDI=inject`,
so the End's `HOOK` can post a keyboard to `/api/graph/midi/inject`. Tried once with a stand-in binary on `:97`.
`lane-suites.sh` already had lane D's `nx` mapping.

**Checks on the joined `next`** (logs in `~/m2-runs/logs/`; `main` hasn't moved from `70f9b61`, so nothing to merge)

| Check | Result | Log |
|---|---|---|
| `cargo build --locked` (server, `m2`) | ok; `Cargo.lock` is lane B's | `nx-merge-build-*`, `nx-merge-build-m2-*` |
| Web build (type-checks) | ok | run by hand in `web/` |
| Rust tests: m2-os, m2-runtime, m2-play, m2-song, m2-session, m2-script, m2-ir, m2-editor, m2-server, music2-desktop, m2 | 1,223 passed, 1 failed (`host_math`, fixed above; then 1 passed) | `nx-merge-rust-tests-*`, `nx-merge-hostmath-*` |
| Clippy, the whole workspace, `-D warnings` | clean; again on m2-play and m2-runtime after the move | `nx-merge-clippy-*`, `nx-merge-clippy2-*` |
| Mac lint: m2-os, m2-runtime, m2-play, m2-session, m2-server, music2-desktop | clean | `nx-merge-maclint-*` |
| Renders | 10 of 10 match `e2e/renders.sha256` | `nx-merge-renders-*` |
| Browser suites, first run | 52 of 56 ok; `s4-meter`, `pl-arrange`, `s8-clips`, `s8-tracks` failed (above) | `nx-merge-suites1-*`, `nx-merge-suites2-*` |
| Browser suites on the tip | 56 of 56 ok, 2,478 of 2,478 checks; `start empty` 14 of 14; `live3b` PART 40 of 60 (no `CLAP_PATH`) | `nx-merge-final1-*`, `nx-merge-final2-*`, `nx-merge-midienv-*` |

The suites on the tip: taste, every `s8-*`, `pl-*` and `nx-*`, and the rest but `live3b`: notes, gestures, g11-play,
fx-fixes, pc-keys, start, newsong, export, undo, attention, effects, s6-builders, s5-knobs, graph, ports, fixes-graph,
fixes-notes, cables, layout, views, code, dsp, s4-clips, s4-attn, s4-problems, s4-meter, live3a, s6-write, s6-fixes.
The full `ci/check.sh` is the End's.

**Cleaned up:** the four lane worktrees (`git worktree remove`) and their branches (`git branch -d`: all merged). The
lanes had deleted their own targets and server copies. `target-nx` and `bin/nx-b7a96c6` stay for the End.

## End

The lead's End.
