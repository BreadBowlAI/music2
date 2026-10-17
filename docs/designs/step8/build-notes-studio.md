# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground: written by the lead.

## Lane A

Audio in the app: written by lane A.

## Lane B

Record what you play: the count-in, the recorder, the Record button, and which MIDI keyboard plays. Branch
`studio-b`: B1 `5c8de3b`, B2 `4b2ae9f`, B3 `3ba146d`, B4 `62039fc`, each green.

**For the merge with P0:** P0 wasn't on `studio` while lane B ran, so lane B made its own seams:
`crates/server/src/routes_record.rs` and `graph/record.rs` (whole files: take lane B's over P0's empty stubs),
`mod routes_record;` after `mod routes_leases;` (`lib.rs`), its `.merge` line after `routes_leases`' (`routes.rs`),
and `pub mod record;` after `mod tempo_lease;` (`graph.rs`: `pub`, as `routes_record.rs` names
`graph::record::Options`). Keep P0's other lines beside them. The take's messages go on the events channel
(`spawn_takes` in `lib.rs`); P0's `AppState.takes` (D9) is unused by lane B and may carry the progress messages
later. `st-record` has no `tips()` yet (P0's `tiplib.mjs`).

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

### B3. The Record button and the take on screen

**In short**

| What | Now |
|---|---|
| The Record button | In the top bar right after Play. A red circle. Tooltip: "Record (R) · right-click for options" |
| R, or the button, when stopped | One bar of clicks. The button counts 4, 3, 2, 1. Then the song plays and recording starts |
| R when playing | Recording starts at once, from the bar you are in |
| While recording | The button is red with a white circle. A red-edged clip grows in the track, with your notes |
| Space | Stops the song and the take. The new clip is selected. "Undo recording on Bass" |
| R while recording | Stops the take. The song plays on |
| Esc, Space, Return or R while counting | Stops the count. Nothing plays |
| Right-click the button | "Count in first (one bar of clicks)", "Snap notes to the grid", "MIDI keyboards…". Kept on this computer |
| The keyboard piano | Records too. Its card says "Plays Bass · recording" |
| Clicking a key in Notes | Heard, not recorded |
| Nothing played | "Nothing was played, so nothing was recorded." |
| Close Song while recording | The take is written first |
| Transport ▸ Record | In the app's menu bar, after Go to Start |

**What changed**

- `web/src/lib/record.ts` (new): the take as the server says it (the state's `record`, then `{"t": "take"}`
  messages); `toggleRecord` (start, stop, or cancel the count), `startRecording` (the play target's lane, the
  options from `settings.json`, Notes' grid in beats when snapping), `stopRecording`, `cancelRecording`, `stopThen`
  (Close Song); the count's beats left from the answer's `{beats, ms}` and the page's clock; a capture-phase key
  listener while counting (Space, Return, Esc, R cancel; D6); the written clip selected once the timeline has it;
  the toasts for `empty` and `error`. Stopped, play's start point moves to where recording starts, so Space goes
  back there.
- `components/RecordButton.tsx` (new): Q17's button (a ghost with a `danger` circle; counting, `danger-soft` with
  the beats left in `danger-strong`; recording, `danger` with a white circle; no animation) and Q18's menu, with
  D34's words. `TopBar.tsx`: it after Play; the time readout now shows from 1212 px (1312 px in the Mac app),
  32 px later, as D39 says.
- `components/Take.tsx` (new): the growing clip, from its bar to the playhead (clamped to the loop's end), a
  1.5 px `danger` outline, `danger-soft` body, "Recording", the notes (a held one grows), drawn every frame from
  `clock.ts`. `SongTimeline.tsx`: one line in `LaneClips`, and `Notes` exported.
- `lib/playTarget.ts`: `holdTarget`: while a take runs, what keys play is its lane (D8). `lib/audition.ts`:
  `strike(…, { record })` marks a strike for the take; `lib/piano.ts`: the piano's strikes carry it.
  `KeyboardPiano.tsx`: "· recording". `lib/settings.ts`: the `record` and `midi` fields.
- `App.tsx`: `r: "record"` in `key()`, `case "record"`, and Close Song stops the take first (D35).
  `Shortcuts.tsx`: Transport "Record what you play: R".
- `apps/music2-desktop/src/menu.rs`: `record` ("Record", no accelerator) after `metronome` in `ITEMS`; Transport:
  after Go to Start; `record_is_in_transport`.
- `e2e/st-record.mjs` (new, 50 checks) and its rows in `run.mjs` and `e2e/README.md`.
- Moved suites (values only, their counts kept, D25): `fx-fixes` and `s8-songbar` (in the Mac app the name also
  goes at 1180 px now; it shows again from about 1200 px), `nx-click` (the Mac app's name checked at 1280 px, not
  1180).

**Tests**

- `st-record` (50 checks): the button's place, size, tooltip and rest look; its menu's rows, checks kept in
  `settings.json` and left out at their defaults, MIDI keyboards… opens Settings, the lint; R: the server counts
  in, the button says 4 then 1, then plays and records; injected keys make a growing take ("Recording", its
  colours); Space writes one clip over whole bars, with the notes at their velocity, one Tx, selected, "Undo
  recording on Bass", ⌘Z takes it back; count-in off kept; the piano records ("Plays Bass · recording") and R
  stops the take with play going on; Notes' key clicks aren't recorded (the toast); snapped to 1/16 and written at
  a loop's wrap, play going on; R while playing records from the bar; Esc and Space while counting cancel and
  nothing plays; a PC page's R; on a capture server, the count's four clicks 24,000 frames apart, the first
  louder, then the song's accented downbeat a bar after the first.
- `menu.rs`: `record_is_in_transport`.
- Moved and green: `taste`, `s8-songbar`, `fx-fixes`, `pc-keys`, `nx-piano`, `nx-midi`, `s8-audition`, `nx-click`.

**Red first**

```
st-record on the page and server before (d-dist-before, bin/d-85b069a):
FAIL  Record sits right after Play, 28 px, and Loop moved 32 px on (play 97+28, record undefined undefined×undefined, loop 129)
FAIL  its tooltip says what it does, its key and its menu: undefined
FAIL  at rest a ghost with a red circle (null)
record_is_in_transport with the rows taken out:
assertion `left == right` failed  left: None  right: Some(("Record", None))
```

**Checks:** the web build; `cargo test -p music2-desktop` and its clippy; the mac lint of `music2-desktop`,
`m2-server`, `m2-play`, `m2-runtime`; the suites above.

**What's left**

- `tips()` (P0's `tiplib.mjs`) isn't in `st-record` yet: P0 wasn't on `studio`. The End's `st-tips` adds the
  Record button and its menu.
- The Play button clicked while counting plays at once with no take (only the keys cancel, D6).

**For gaps.md:** #24: Record in the app (R, the button, Transport ▸ Record); #19: a count-in before recording.

**Contract:** taste.md §3.3 (Record after Play at 144, everything right of it 32 px further), §2.6 (`danger` marks
recording, D18), §3.8 (the time shows from 1212 px, 1312 in the Mac app; there the name goes up to about 1200
px), §5.1 (R is Record), §5.3 (Transport ▸ Record).

### B4. Which MIDI keyboard plays

**In short**

| What | Now |
|---|---|
| Settings (⌘,) | A new row, MIDI keyboards, after Buffer size |
| Each keyboard | Its name and a "Plays" box, ticked at first |
| Untick it | That keyboard plays nothing and records nothing. The top bar's mark says it is off |
| A keyboard turned off and unplugged | Its row stays: "Keys (not connected)" |
| None | "No MIDI keyboard is plugged in." |

**What changed**

- `components/SettingsDialog.tsx`: the MIDI keyboards row (`Keyboards`): one row per keyboard plugged in or turned
  off, its name, "(not connected)" for one gone, and a "Plays" check box as Rename Song's (D22); a change posts
  `{midi: {off}}` at once. The server's B2 side keeps it and tells every window (`midi` event).
- `lib/midi.ts`: the `midi` state keeps `off` (from the state's `midi` and the `midi` event).
- `components/MidiStatus.tsx`: the mark's tooltip names only the keyboards that play, and then "Pads is turned off
  in Settings".
- `e2e/st-record.mjs`: 11 more checks (61 in all); its rows in `run.mjs` and `e2e/README.md` follow.

**Tests**

- `st-record`'s Settings part: ⌘, opens Settings; a row for "Test Keys", Plays ticked; unticked, kept in
  `settings.json`; the lint of the dialog; the mark says it is off; a key plays nothing (`live` 0) and a take
  records nothing (the toast); unplugged, "Test Keys (not connected)"; ticked again, left out of the file, and a key
  plays (`live` 1); none plugged in and none off, "No MIDI keyboard is plugged in."
- Moved and green: `nx-settings` (42), `nx-midi` (24), `taste`.

**Red first** (st-record on B3's page):

```
FAIL  a row for the keyboard plugged in, Plays ticked ()
Error: No element found for selector: [data-keyboard='Test Keys'] input
```

**What's left:** nothing of B4.

**For gaps.md:** #31: MIDI keyboards in Settings: which ones play (an input device for recording audio is left,
with #25).

**Contract:** FMT-20: `settings.json` `midi: {off: [names]}` (by the names `m2_os::midi` gives); taste.md §3.9:
Settings' MIDI keyboards row.

## Lane C

MIDI files and stems: written by lane C.

## Lane D

Plain-language help: written by lane D.

## Merge

Written by the lead.

## End

Written by the lead.
