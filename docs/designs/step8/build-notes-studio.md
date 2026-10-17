# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground: written by the lead.

## Lane A

Audio in the app: written by lane A.

## Lane B

Record what you play: the count-in, the recorder, the Record button, and which MIDI keyboard plays. Branch
`studio-b`.

### B1. The count-in, in the engine

**In short**

| What | Now |
|---|---|
| A count-in | The engine can click one bar, then play the song. The first click is higher and louder |
| When the song starts | Exactly one bar after the first click, on the next block line |
| Space, Stop, a jump or an export during the count | The count stops. Nothing more clicks |
| The metronome off | The count still clicks. The song then plays with no click |
| Exports | Never count in. Nothing they make changes |

Nothing in the app uses it yet: that is B2 and B3.

**What changed**

- `core/runtime/src/click.rs`: `Click` gains a count (plain fields: beats, frames per beat, frames done, its lead-in,
  its length, and an end flag). `count_in(beats, frames_per_beat)` lays the bar so it ends on a block line: click
  `k` at `round(k × frames_per_beat)` after the first, the bar `round(beats × frames_per_beat)` frames, the first
  click `ceil(bar / BLOCK) × BLOCK − bar` frames into the first block (D27). `mix` makes the count's blips from
  its own frames, the first accented; `counting()`, `cancel_count()`, and `count_ended()`, true once, for the block
  the bar ends with.
- `core/runtime/src/engine.rs`: `Msg::CountIn { beats, frames_per_beat }`, applied at once as a transport command
  is. A Play, a Stop or a seek read while counting cancels it (D6); so does a hold, and a count read while held does
  nothing. The engine never idles while counting. After the click's mix, a count that ended starts the transport
  (`Cmd::Play`), so the song's first frame is the block after the bar.
- `core/play/src/lib.rs`: `Player::count_in(beats, frames_per_beat) -> Result<u64>` (its seq: `applied` says the
  block it started at). The `CountIn` arm where nothing plays.

**Tests**

- `core/runtime/src/click/tests.rs`: `a_count_in_clicks_one_bar_then_plays` (120 bpm 4/4 stopped at beat 8, the
  metronome on: clicks at 0, 24,000, 48,000, 72,000; at frame 96,000 the transport plays from beat 8's frame, and
  its downbeat clicks there, accented), `a_count_in_follows_the_meter` (3/4: the first click 64 frames in, the bar
  ending on the line at 72,064; 6/8: six clicks 12,000 apart; a step of 20,000.4 frames rounds each click),
  `the_count_accents_its_first_click`, `stop_cancels_a_count_in` (a Stop, a seek and a hold),
  `play_while_counting_cancels_the_count` (D6: no second Play at the bar's end), `a_count_in_wakes_an_idle_engine`.
- `core/play/tests/no_alloc_count.rs`: `the_count_in_never_allocates` (the count, the song after it, and a second
  count stopped half way, under the trap).

**Red first** (`Click::count_in` a stub that does nothing):

```
---- click::tests::a_count_in_clicks_one_bar_then_plays stdout ----
panicked at core/runtime/src/click/tests.rs:228:5: playing once the bar is over
---- click::tests::a_count_in_follows_the_meter stdout ----
assertion `left == right` failed  left: []  right: [64, 24064, 48064]
---- click::tests::play_while_counting_cancels_the_count stdout ----
assertion `left == right` failed: no click after the Play  left: []  right: [0, 24000]
```

**Guards:** `no_count_in_changes_no_bit` (no count, or one cancelled in its first block before its first click: the
export's bits). `the_count_in_never_allocates` is a guard too: the code it watches never allocated.

**Checks:** `cargo test -p m2-runtime -p m2-play`, clippy of both; the renders: 10 of 10.

**What's left:** nothing of B1.

**For 8a's merge**

| File | Block |
|---|---|
| `core/runtime/src/click.rs` | The count's six fields at `Click`'s end; `count_in`, `counting`, `cancel_count`, `count_ended` after `is_on`; in `mix`, `&& !self.counting()` in the first test and the one `if self.counting()` branch before `if self.on` |
| `core/runtime/src/click/tests.rs` | Seven tests at the file's end |
| `core/runtime/src/engine.rs` | `Msg::CountIn` at the enum's end and in `structural()`; in `apply_now`, the cancel in the `Transport` arm and the two `CountIn` arms; the hold's arm in `try_apply` (a hold cancels the count) and `CountIn` in its last arm; in `render`, `&& !self.click.counting()` on the idle check and on the idle-after-silence check, and the three lines after `self.click.mix(…)`. Nothing in the `Err(e)` arm |
| `core/play/src/lib.rs` | `Player::count_in` after `Player::click`; the `CountIn` arm in `take_held`'s inner match |
| `core/play/tests/no_alloc_count.rs` | New |

**For gaps.md:** #19's count-in: built in the engine (B1), used by Record (B3).

**Contract:** RENDER-28: the count-in is plain fields in `Click`, no allocation; it never reaches an export
(RENDER-1, RENDER-3), as the metronome doesn't.

### B2. The recorder, on the server

**In short**

| What | Now |
|---|---|
| Record on a track with notes | The server keeps what you play on a MIDI keyboard, or on the keyboard piano |
| Stopped | One bar of clicks first (or none), then the song plays and recording starts where play started |
| Playing | Recording starts now, with no clicks |
| Where each note goes | Where you heard it, between the sound card's buffers, not on them |
| A very quick tap | Still a note: at least 1/64 of a beat |
| Stop (Space), R, a click on the ruler | The take ends where you were. Notes you still hold end there |
| Looping | Only the first time round is kept. At the loop's end the take is written; the loop plays on |
| What is written | One notes clip over whole bars, at least one. One Undo step: "Undo recording on Bass" |
| Nothing played | Nothing is written |
| A MIDI keyboard turned off (Settings) | It plays nothing and records nothing |
| Clicking another track while recording | The keys stay on the track you record |

Nothing in the app uses it yet: that is B3. Suites can reach it through the routes.

**What changed**

- `core/play/src/lib.rs`: `Player::heard_frame()` (D4): the engine clock and the transport frame the speakers play
  now, from the last callback, the time since it began, less two device buffers in engine frames.
  `heard_frame_at(ns)` for tests.
- `crates/server/src/graph/record.rs` (new): `Recorder` on the control plane (the take, and the takes waiting to be
  written). `Graph::record_start` (refused with no notes lane, a take already running; stopped: a count-in from the
  tempo and meter there, or a Play; playing: from now), `record_stop`, `record_cancel`, `recording`,
  `record_tick` (the take thread's: a count that turned into playing or never did, the loop's end, a new player,
  then each take written as one `Gesture::Set` Tx and named "recording on Bass"). A note's beat: from the
  transport's frame (exact) unless it is more than two blocks from the clock's (a wrap or a count), then from the
  engine clock and the take's anchor (the clock where the transport was at the start: the count's or the Play's
  `applied` stamp plus the count's length). Quantize uses `Gesture::Quantize`'s own rounding (`ends: false`).
- `graph.rs` (B's blocks): `pub mod record;`, `ControlPlane.record`, `state["record"]` after `state["midi"]`, one line
  at the top of `transport` (while counting anything cancels, and a toggle is a Stop; while recording a Stop or a
  toggle ends the take) and one in `seek` (a jump ends it). `stop_to` calls `transport`, so it is covered.
- `graph/audition.rs`: `Audition.record` (serde default false); each On sent from a keyboard or a window marked
  `record`, and each Off, goes to the take; a release ends a source's notes in it.
- `graph/midi.rs`: `Midi.off` (keyboards turned off: nothing plays or records); while a take runs, keys play its
  lane (D8); `set_midi_off`; the state's `midi.off` when some are; the toast for one plugged in while off.
- `settings.rs`: `record: {count_in, quantize}` and `midi: {off}`, left out of the file at their defaults.
  `routes_settings.rs`: `GET` answers both, `POST` takes both (a `midi` change also tells every window).
- `routes_record.rs` (new): `POST /api/graph/record/start {session, module, lane, count_in?, quantize?}`, `/stop`,
  `/cancel`; writes (423 read-only); start refused while an export runs ("Wait for the export to finish.", D26).
- `lib.rs`: `mod routes_record;`, the `music2-takes` thread (every 100 ms, `record_tick`; what it says goes to every
  window on the events channel, and a written take is followed by the graph event), and `keep_settings` applies the
  keyboards turned off. `routes.rs`: its `.merge` line.

**Tests**

- `crates/server/src/graph/record_tests.rs` (a driven player; each note's window is known from the callback before
  it and the test's own time after): `a_take_records_midi_notes_where_they_were_heard`,
  `notes_land_between_callbacks`, `a_quick_tap_still_records_a_note`,
  `piano_notes_marked_record_are_recorded_and_audition_is_not`, `stop_writes_one_tx_named_recording_on_bass`,
  `held_notes_end_at_the_stop`, `the_take_spans_whole_bars`, `an_empty_take_writes_nothing`,
  `quantize_puts_starts_on_the_grid` (the same notes as `Gesture::Quantize`), `a_loop_wrap_writes_the_first_pass`,
  `a_note_after_the_wrap_is_not_put_at_the_start`, `a_note_held_over_the_wrap_ends_at_the_loop_end`,
  `a_click_on_the_ruler_ends_the_take`, `a_count_in_then_record_starts_at_the_start_point`,
  `a_note_in_the_counts_last_beat_lands_on_the_first_beat`, `a_keyboard_turned_off_neither_plays_nor_records`,
  `record_needs_a_notes_lane`, `closing_the_song_drops_the_take`, `the_keys_stay_on_the_takes_lane`,
  `space_while_counting_cancels_and_nothing_plays`, `r_while_playing_records_from_now`.
- `crates/server/src/routes_record_tests.rs`: `a_take_through_the_routes` (start, a key, stop over HTTP; the take
  thread writes it and every window hears `written`, then `graph`), `a_read_only_window_cant_record` (423).
- `settings.rs`: `record_options_and_keyboards_are_kept`.

**Red first**

```
heard_frame without the time since the callback (callback steps):
---- graph::record::tests::notes_land_between_callbacks stdout ----
panicked at record_tests.rs:196:5: 5 ms after the callback's line 1: written at 1
beat from the transport's frame only (today's wrapping position):
---- graph::record::tests::a_note_after_the_wrap_is_not_put_at_the_start stdout ----
assertion `left == right` failed: {…"notes":[{…"start":0.10004166666666667…},{…"start":2.0000416666666667…}]…}  left: 2
Recorder::on a no-op:
---- graph::record::tests::a_take_records_midi_notes_where_they_were_heard stdout ----
panicked at record_tests.rs:165:5: {"empty":true,"t":"take"}
```

**Guards:** `stopping_never_waits_for_a_compile` (D7: the actor held by a slow job, a Stop answers within 200 ms).

**Checks:** `cargo test -p m2-server -p m2-play`, clippy of `m2-server`, `m2-play`, `m2-runtime`.

**What's left**

- A note struck while the audio callback runs (a fraction of a millisecond each buffer) is placed up to one buffer
  early: `heard_frame` reads the callback's start time before its clock is published. Two buffers of output delay
  is a guess; HANDOFF's Mac step checks it by ear.
- Progress goes on the events channel (`{"t": "take", …}` when the phase or the notes change). P0's
  `AppState.takes` channel (D9) wasn't on `studio` yet; the integrator may move it there.

**For gaps.md:** #24: recording notes is built (a take per pass, first pass only; overdub, one clip per pass, pitch
bend and the mod wheel left). #31: which MIDI keyboards play is a setting.

**Contract:** MON-7: a take reads live notes on the control plane, never through the session; RENDER-29: Record,
Stop and a jump never wait for a compile, the take thread writes; WRITE-10: a take is one `Clips` Tx; FMT-20's
`settings.json` gains `record` and `midi`.

**For 8a's merge**

| File | Block |
|---|---|
| `core/play/src/lib.rs` | `Player::heard_frame` and `heard_frame_at`, right after `Player::frames` |

## Lane C

MIDI files and stems: written by lane C.

## Lane D

Plain-language help: written by lane D.

## Merge

Written by the lead.

## End

Written by the lead.
