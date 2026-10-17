# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

P0 never reached `studio` while the lanes ran. Lane A built the P0 pieces it needed to the plan's spec
(`tracks::add_tracks` with `Play` and `NewTrack`, the drop layer, the app's `open_file`), and each lane made its own
seams (`mod` and `.merge` lines). The merge kept one copy of each: see Lane A's table and the Merge section.

## Lane A

Not merged yet.

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

### B2 fix. R right after a jump

**In short**

| What | Now |
|---|---|
| R pressed while the song is still jumping to a new place (a few milliseconds) | "The song is moving to another place. Press Record again." Nothing records from the old place |

**What changed:** `Graph::record_start` refuses while playing with a seek the audio thread hasn't taken
(`status().pending`): the place it would record from is about to change.

**Tests:** `r_right_after_a_jump_asks_again` (a driven player: play, seek with no callback between, R; then two
buffers on, R records from the new bar).

**Red first** (the refusal taken out):

```
called `Result::unwrap_err()` on an `Ok` value: Object {"count_in": Null, "from": Number(0.16004166666666667), … "phase": String("take"), "start": Number(0.0) …}
```

**Checks:** `cargo test -p m2-server` (328 and 1), its clippy.

**What's left:** right after a jump, for two buffers, the speakers still play the old place, so a take started then
starts at the bar before the new place.

## Lane C

MIDI files in and out, and stems from the Export dialog (gaps #36). Branch `studio-c`, worktree `wt/st-c`, from
`studio` at `85b069a` (P0 hadn't landed: lane C made its own module stubs, as the plan's §2.1 names them, and
the merge joins them).

**For 8a's merge:** lane C changes nothing in `core/runtime`, `core/play` or `core/os`.

**Where lane C stands**

| Slice | State | Commit |
|---|---|---|
| C1. MIDI files read and written; Export as MIDI… | Done | `6aacaa2` |
| C3a. Stems | Done | `3e39876` |
| C3b. The Export dialog | Done | `24545dd` |
| C2a. MIDI files read in; one part into a notes lane | Done | `f26410e` |
| C2b. New tracks from a MIDI file, Import MIDI…, drops, the tempo question | Left: needs P0 (`tracks::add_tracks`, the drop layer, `open_file`, `droplib.mjs`), which never reached `studio` while lane C ran | — |

C3 came before C2's second half for that reason (the plan's order was C1, C2, C3).

**For the merge with P0** (the integrator): P0's stubs and lane C's files meet at
- `crates/editor/src/{smf.rs, midifile.rs}`, `crates/server/src/{routes_exchange.rs, stems.rs, graph/exchange.rs}`:
  P0 makes them empty, lane C fills them: take lane C's.
- `crates/editor/src/lib.rs` (`pub mod smf; pub mod midifile;` after `rekey`), `crates/server/src/lib.rs`
  (`mod routes_exchange; pub mod stems;` after `routes_leases`), `routes.rs` (`.merge(routes_exchange)` after
  `routes_leases`'), `graph.rs` (`pub(crate) mod exchange;` after `tempo_lease`: lane C needs it `pub(crate)` for
  the route's `MidiSource` and `MidiTarget`). Join P0's neighbouring lines.
- `crates/editor/src/render.rs`: lane C's `rooted`, `bounce_seconds`, `write_float` and its `float` field; P0's (and
  lane A's) three `pub(crate)` words on `part_path`, `sweep`, `name_new`.
- `web/src/App.tsx`: `<ExportDialog />` after `<RenameSongDialog />` and its import at the import list's end.
  `web/src/components/TopBar.tsx`: one hunk in `ExportButton` (its two titles).
- `apps/music2-desktop/src/menu.rs`: `export`'s title "Export…" and its test at the module's end; lane A adds
  `import_audio` near it.
- `e2e/run.mjs`, `e2e/README.md`: `st-midi` (19) and `st-stems` (49) after `export`.

### C1. MIDI files, read and written; Export as MIDI…

**In short**

| What | Now |
|---|---|
| Right-click a notes clip | The menu has **Export as MIDI…**, after Repeat… |
| Pick it in the app | A save panel opens at `Exports/<song> <Track>.mid` |
| Pick it in a browser | The file goes straight to the songs folder's Exports, with a free name |
| Afterwards | A toast: "Saved Bass as a MIDI file: first-song Bass.mid", with Show in Finder in the app |
| The file | Other music apps open it: the clip's notes from its start, its tempo and time signature, and a fitting General MIDI sound |

**What changed**

- `crates/editor/src/smf.rs` (new): Standard MIDI Files read and written, no crate (Q31, D10, D37). `read` takes
  formats 0 and 1 in ticks a beat: notes (a Note On at velocity 0 is an Off), programs, tempo, time signatures, each
  track's first name and its end. Running status carries across meta and sysex events; sysex, controllers, other
  meta events and unknown chunks are skipped. Refused in words that name the byte: no `MThd`, SMPTE time, format 2,
  a division of 0, a length over 4 bytes, a chunk or an event past the file's end, data with no status, over 16 MB,
  over 200,000 notes. `SmfTrack::notes` pairs Ons and Offs: a note struck again before its Off ends there, one with
  no Off ends at the track's end, an Off with no On is ignored; a tap (On and Off at one tick) stays, an On repeated
  at its own tick goes. `write`: full status bytes, each track named first, and at one tick tempo and meter, then
  programs, then Offs, then Ons.
- `crates/editor/src/midifile.rs` (new): Q33's table, one for both ways (`sound_for` picks a std sound by channel
  and program, or by pitch with no program; `program_for` the program a sound is written with), `player_of` (the
  instrument that reads a lane, from the frames of the lane's readers, as `note_names` finds its names),
  `export_song` (Q37: format 1 at 960 a beat; the song's name, tempo map and time signatures first; a track per
  notes lane with notes, in the tracks' order, named "Bass" or "Bass · Top" for a second lane; drums on channel 10,
  the rest 1–9 and 11–16; notes cut at their clips' edges as they play; velocity `round(v × 127)` in 1–127, pitch
  rounded; an overlap of one pitch ends the first where the second starts; tracks muted in the song left out;
  refused in words with no notes to write) and `export_clip` (one clip from tick 0, with the tempo and meter where it
  starts and the changes inside it).
  - The table writes each picked sound with a fitting General MIDI program (FM keys 4, FM bell 14, Synth pad 89,
    Synth pluck 25, Synth bass 38, Strings 48, Supersaw 62, Lead 81, FM metal 114), so a file read back picks the
    same sound. It also writes std/instruments' `bass`, `pluck` and `pad` (38, 25, 89) and 808 sub (38), which no
    program picks. std/instruments' `kick` and `hat` have no category, but play drums: their lanes go on channel 10
    too.
- `crates/server/src/graph/exchange.rs` (new): `Graph::midi_file(token, clip)`, on the actor: the bytes and the
  file's base name ("first-song", "first-song Bass").
- `crates/server/src/routes_exchange.rs` (new, its own router and read-only middleware, as `routes_view.rs`):
  `POST /api/graph/midifile/where {session, clip?}` → `{path}` (where the app's save panel starts: the first free
  name in Exports, which is made) and `POST /api/graph/midifile/export {session, path?, clip?}` → `{path}`. A picked
  file is written beside and renamed over (replaced, as Export Audio replaces one; `.mid` added when missing); a new
  one is named create-only, the next number when a name is taken meanwhile. Neither writes the song, so a
  read-only window may.
- `crates/server/src/{lib.rs, routes.rs, graph.rs}`: the `mod` and `.merge` lines P0's stubs would have
  (`routes_exchange` after `routes_leases`, `exchange` after `tempo_lease`).
- `web/src/lib/midiFiles.ts` (new): `exportMidi(clip?)`: in the app, the route's start path, then P0's
  `save_file` (`{title, name, start, filters}`); in a browser, straight to Exports. The toast as above, with Show in
  Finder (Show in Folder off the Mac) in the app.
- `web/src/components/SongTimeline.tsx` (C's hunk in `TimelineMenu`): **Export as MIDI…** after Repeat… (pl-arrange
  wants Repeat… right after Duplicate), only when one notes clip is picked. Its import at the import list's end.

**Tests**

- `smf_tests.rs` (12): `reads_the_spec_example` (SMF 1.0's own example, as format 0 and as format 1: its programs,
  tempo, meter and four notes), `reads_format_0_and_1`, `running_status_and_sysex`, `velocity_0_ends_a_note`,
  `a_note_without_an_off_ends_at_the_next_on`, `tempo_and_meter_events`, `an_smpte_division_is_refused_in_words`,
  `format_2_is_refused`, `a_cut_short_file_says_where`, `too_big_or_too_many_notes_is_refused_in_words`,
  `writes_what_it_reads` (both spec files, and the bytes of a written track), `a_mangled_file_never_panics`
  (every prefix of the spec file, then 2,000 seeded files with bytes flipped).
- `midifile_tests.rs` (9): `the_song_has_a_tempo_track_and_a_track_per_lane` (first-song: Kick and Hats on channel
  10, Bass and Keys on 1 and 2 with programs 38 and 25, every note of the song), `a_second_lane_is_named_by_its_track_and_lane`,
  `a_drum_kit_lane_is_channel_10`, `notes_are_cut_at_their_clips_edges` (and two clips laid over each other),
  `muted_in_song_tracks_are_left_out`, `velocity_and_pitch_are_rounded_into_midi`, `a_clip_round_trips`,
  `a_clip_later_in_the_song_takes_the_tempo_where_it_starts`, `the_sound_table_picks_and_writes_one_way`.
- `routes_exchange_tests.rs` (2): `the_song_and_a_clip_are_written_where_picked` (a picked file replaced, `.mid`
  added, a clip from its start, a gone clip and another session refused in words, no part file left),
  `a_new_midi_file_never_replaces_one`.
- Suite `st-midi` (kind `fix`, 14 checks so far): the row after Repeat…, its words, the menu's taste lint; a browser
  page writes `first-song Bass.mid` to `~/Music/music2/Exports` with the toast; the file reads back (in node) to the
  clip's 48 notes on channel 1; a second export is `first-song Bass 2.mid`, the same bytes; an automation clip and
  two picked clips have no row.
- Moved: `s8-clips` (the clip menu's rows gain "Export as MIDI…" after "Repeat…": the same check, its value only).
  `pl-arrange` (Repeat… still right after Duplicate) and `taste` pass unchanged.

**Red first**

- The Rust tests against stubs (`read` answers an error, `write` and both exports answer empty bytes): 19 of 21
  fail, e.g. `smf_tests.rs:82:35` (`read(&hex(SPEC_0)).unwrap()` on an `Err`), `midifile_tests.rs:17:32`
  (`smf::read(bytes).unwrap()` of empty bytes), `midifile_tests.rs:188:5`.
- The route test without the router merged: `left: 404, right: 200` at `routes_exchange_tests.rs:62`.
- `st-midi` on the before page and server (`85b069a`):
  `FAIL  a notes clip's menu has Export as MIDI… after Repeat…: cut, copy, paste, duplicate, repeat, split, quantize, delete, loop, open`,
  then the suite stops: `failed to find element matching selector "[data-testid=clip-menu] [data-menu-item="export-midi"]"`.

**Guards**

- `a_mangled_file_never_panics` (a stub never panics either) and `the_sound_table_picks_and_writes_one_way` (a table).

**What's left**

- The app's save panel needs P0's `save_file` action; until the merge, the app's row would answer "unknown action"
  in its toast. The browser path is what st-midi checks; C2 adds the app page's.
- The tiplib check (`tips()`) is P0's: added to st-midi once `studio` has it.

**For gaps.md:** #36: a notes clip saves as a MIDI file (Export as MIDI…), with a fitting General MIDI sound and
drums on channel 10.

**Contract:** `smf.rs` reads SMF formats 0 and 1 in ticks a beat and refuses the rest in words; music2 writes format 1
at 960 ticks a beat. A MIDI file is written outside the song only (Exports, or a picked file).

### C2a. MIDI files read in; one part into a notes lane

C2 needs P0's `tracks::add_tracks` (new tracks in one Tx), drop layer, `open_file` and `droplib.mjs`, none of which is
on `studio` yet. This half is what needs none of them: the file read into parts with their sounds, its tempo map
and time signatures, the tempo question's rule, and a one-part file brought into a notes lane, with its route. The
rest is C2b (What's left).

**In short**

| What | Now |
|---|---|
| A MIDI file from another app | music2 reads it: each part (a track's notes on one channel) with a fitting sound |
| Drums (channel 10) | The Drum kit |
| Other parts | By their General MIDI sound: pianos play FM keys, basses Synth bass, strings Strings… With no sound named: Synth bass for low parts, else FM keys |
| A one-part file onto a notes lane | One clip there, to the bar line after its last note, one Undo step: "Undo importing riff.mid". The song's tempo stays |
| Not a MIDI file, no notes, over 32 parts, past bar 9,999 | Refused, in words. Nothing is changed |

The app reaches this with C2b's drop. Until then, the route does (`st-midi` uploads as a browser does).

**What changed**

- `crates/editor/src/midifile.rs` (C's):
  - `bring(file, name, label_of) -> Brought { parts, arrangement }` (Q32–Q34, D10): every pair of a track and a
    channel with notes, in file order, the channels in the order their first notes come (a format 0 file splits by
    channel); the sound by `sound_for` (channel 10, else the first program the track, then any track, sets on the
    channel, else the average pitch); the name: the track's when it makes one part, else the sound's label, a repeat
    numbered ("Piano 2"); notes in beats by the division, velocity `v / 127` to a thousandth (so `round(v × 127)`
    gives it back), at least 1/64 beat. Refused in words: no notes, over 32 parts, a note past bar 9,999.
  - `arrangement_of` (D10): every track's tempos and meters by tick, the last at one tick winning; tempos to a
    hundredth of a BPM (483871 µs is 124, as music2 writes it) and clamped into 20 to 999; 120 until the first; a
    meter outside 1 to 64 over 1 to 32, or not on a bar line of the one before, skipped, and one on a bar line put
    exactly there; 4/4 alone is none written. What comes out passes `Arrangement::read`.
  - `tempo_plan(song_empty, song, file, choice)` (Q35): into an empty song, the file's tempo map and time signatures;
    else nothing when the tempos are the same or the answer is Keep, the file's tempo map (the song's meters kept)
    for Use, and `Asks { file_bpm, song_bpm }` with no answer. C2b's Tx writes what it says.
  - `clip_length(a, at, end)`: from `at` to the bar line at or after its last note, a bar at least.
  - `import_into(s, file, name, module, lane, at)` (Q36, D11): a one-part file onto a notes lane at `at`, as one
    `Gesture::Set` (one `Clips` Tx), by the song's own tempo and meter (no question). Refused in words: more than one
    part ("groove.mid has 3 parts: drop it below the tracks to bring each in as a track of its own."), a lane that
    isn't notes.
- `crates/server/src/graph/exchange.rs`: `Graph::import_midi(token, source, target)`: the file read off the actor
  (a path over 16 MB refused before it is read), then one actor job, the step named "importing riff.mid" on the
  server. `MidiSource` (a path, or bytes with a name) and `MidiTarget` (`{kind: "lane", module, lane, beat}`;
  C2b adds `{kind: "below", beat}`). `graph.rs`' line is `pub(crate) mod exchange;` so the route names them.
- `crates/server/src/routes_exchange.rs`: `POST /api/graph/midifile/import`: JSON `{session, path, to}` (the app) or
  the bytes with `?session=&name=&to=` (a browser), its body limit 16 MB (D10). A write: a read-only window gets 423.

**Tests**

- `midifile_tests.rs` (11 more): `each_track_and_channel_becomes_a_track`, `channel_10_plays_the_drum_kit`,
  `programs_pick_std_sounds` (one program of each family, and a program set on another track),
  `no_program_picks_by_pitch`, `notes_shorter_than_a_64th_get_a_64th`, `odd_tempos_and_meters_stay_in_range` (10 and
  60,000,000 BPM, two tempos at one tick, a 0-beat meter, a unit of 512, a meter off the bar line; the result reads
  as an arrangement), `more_than_32_parts_is_refused` (and 32 taken; no notes; bar 9,999 and 10,000),
  `an_exported_song_imports_as_the_same_notes` (first-song written by C1, read back: the names, the sounds, 124 BPM,
  every note as beats), `an_empty_song_takes_the_files_tempo`, `a_song_with_clips_asks_then_keeps_or_takes_the_tempo`,
  `a_one_track_file_dropped_on_a_notes_lane_goes_into_it` (a two-bar clip at bar 9 by beats, no tempo written, one
  undo step back to the same bytes; two parts and an automation lane refused in words).
- `routes_exchange_tests.rs`: `a_midi_file_comes_into_a_notes_lane_as_one_undo_step` (a path and an upload; the
  step's name; a text file refused with nothing written).
- `st-midi` grows to 19: an upload of a one-part file into Bass at bar 9 (its answer, the two-bar clip of two notes,
  "Undo importing riff.mid", ⌘Z), and a text file named `.mid`. Its MIDI files are made in node by `smfBytes`, which
  moves to `droplib.mjs`' `smf()` with C2b.

**Red first**

- Against stubs (`bring` bringing nothing, `tempo_plan` writing nothing): 11 of the 11 new editor tests fail, e.g.
  `midifile_tests.rs:291` (no parts), `:420` (the empty song's tempo), `:444` (`import_into` refused); the route test
  `left: 400, right: 200` at `routes_exchange_tests.rs:144`.
- `st-midi` on C3's server (no import route): `FAIL  riff.mid comes into Bass's notes at bar 9: {"error":"no such API"}`,
  `FAIL  Undo names it: Undo (⌘Z)`, `FAIL  a text file named .mid says so: {"error":"no such API"}`.

**Guards**

- None.

**What's left** (C2b, after P0 lands on `studio`)

- New tracks: `import_new` building P0's `NewTrack { play: Play::Instrument(part.component), label, key, clips }` for
  each part (one clip from the bar, `clip_length` long, by the arrangement in effect) and one
  `tracks::add_tracks(…, tempo_plan's arrangement)`: one Tx, one undo step "importing groove.mid"; the route's
  `{kind: "below", beat}` and its `{asks: {file_bpm, song_bpm}}` answer, and `tempo: "keep" | "use"`.
- `components/MidiTempoAsk.tsx` (the question as a page dialog: "groove.mid is at 128 BPM. The song is at 120 BPM."
  [Keep 120] [Use 128], D34's second line), File ▸ Import MIDI… (`menu.rs`' `import_midi` after A's `import_audio`;
  P0's `open_file` in the app, a file input in a browser), `App.tsx`' `case "import_midi"`, and `lib/midiFiles.ts`
  registering `.mid` and `.midi` with P0's drop layer (hints "groove.mid: 3 tracks · bar 1", "Into Bass · bar 3").
- `st-midi`'s import part through the menu and a drop (D11), `smf()` in `droplib.mjs`, the toast "Imported
  groove.mid: Drums, Bass and Keys" with Undo.

**For gaps.md:** #36: MIDI files read (formats 0 and 1); a one-part file goes into a notes lane. New tracks from a
MIDI file wait for C2b.

**Contract:** a MIDI part is a track and a channel with notes; channel 10 plays the Drum kit, others by General MIDI's
families (Q33). An import into a lane keeps the song's tempo (D11).

### C3a. Stems

**In short**

| What | Now |
|---|---|
| Stems | Each track of the song becomes its own WAV file, for another music app |
| Where | A new folder in the songs folder's Exports: `first-song stems`, then `first-song stems 2` |
| Names | `1 Kick.wav`, `2 Hats.wav`… in the tracks' order |
| What is in each | The track alone, before its mixer channel: no fader, no pan, no master. 32-bit float, to 2 s past the last clip |
| Muted in the song | Left out |
| A song that plays plugins | Not yet: "Not yet for a song that plays plugins (Surge XT)." |
| While a song exports | Stems wait their turn: one export at a time, of either kind |

The Export dialog that offers them is C3b. Until then they are reached through the API only.

**What changed**

- `crates/editor/src/render.rs` (lane C's this track): `rooted(ir, track, export)` (moved out of `m2 bounce`'s arm:
  the song played through a node of the root that imports the track's export, ID-3), `bounce_seconds(data)` (to 2 s
  past the last clip, `m2 bounce`'s default), and `Render::write_float(path, progress)`: 32-bit float, two channels,
  every sample as rendered, nothing clipped (D15), through a part file, named create-only. `Render` gains one field,
  `float`; `Out::push` writes floats when it is set. Nothing 8a changed there (`Threads::from_env`, `parse`) is
  touched.
- `apps/m2/src/main.rs` (the bounce arm): calls `render::rooted` and `render::bounce_seconds`. Its files keep every
  byte: `m2 bounce first-song stem-<key> --track <key>` for kick, hats, bass and keys, before (the `85b069a` code)
  and after, both `sha256sum -c` OK:

  | File | SHA-256 |
  |---|---|
  | `stem-kick.wav` | `5d13254850824725bb41953825bd171f8a5651fd201738b45594093819facec6` |
  | `stem-hats.wav` | `156c8ea54b845a213b64682f32c3b81e115e0b35ad29f2d2c14d016e473997e3` |
  | `stem-bass.wav` | `c09c2e6797f2fe6a7e9bb0134c8976fb7c67d227086c81c0e61cbfcbfef81f14` |
  | `stem-keys.wav` | `a36080c08101e33f0065c2ca3b3cd3695134c8f14052fb10eba34b416c37d841` |

  (and their `.wav.json` provenances, the same too.)
- `crates/server/src/graph/exchange.rs`: `Graph::stems(token)` plans on the accepted revision (the tracks in the
  timeline's order, muted ones and ones with no `out` left out, each `rooted` and `m2_ir::check`ed); refused in words
  for a song with a plugin node (Q40: the plugins' names, an uninstalled one by its id) or with no track left.
  `Graph::stem_render(plan, i)` builds stem `i`'s `Render` on the actor from the planned revision, on an export's
  threads (`export_threads`), from 0 for `bounce_seconds`. `Graph::stems_refused(token)`: the words, or none.
- `crates/server/src/stems.rs` (new): `stems::start(state, token)`: the export's slot, the plan, a new folder
  (`<song> stems`, `… stems 2`, made create-only and synced), then a thread that renders the stems one after another
  with `write_float` into `<n> <Label>.wav`. Events as an export's, with `stems: true`: `progress` (0..1 for all)
  with `stem`, `n` and `of`; `done` with the folder's `path`, `count` and `seconds`; or `error`, after which the
  stems it wrote and the folder are gone. Quitting stops it (the slot's `stop`).
- `crates/server/src/export.rs` (C's block, after `Exports`' impl): `Slot`, the one export slot taken by another
  kind of job: `Slot::take(state, song)`, `stop()`, and its `Drop` frees it. An export and stems refuse each other
  with the export's own words.
- `crates/server/src/routes_exchange.rs`: `POST /api/graph/stems {session}` → `{path}` and `POST
  /api/graph/stems/check {session}` → `{why}`. `crates/server/src/lib.rs`: `pub mod stems;`.

**Tests**

- `render.rs`: `a_stem_renders_what_m2_bounce_renders` (each of first-song's four tracks: `Render` with `rooted` and
  `write_float` gives the same file as bounce's own loop: `Renderer::new`, `seek_on`, `render_on`, `write_new_wav`),
  `a_float_render_is_never_clipped_and_never_replaces_a_file` (a sine at 3× full scale keeps its peak; a second write
  to the name is refused and changes nothing).
- `apps/m2/tests/bounce.rs` (new, its own binary: `render.rs`' test sets the environment as the only test there):
  `bounce_of_a_track_still_writes_its_file` (`m2 bounce --track bass`: 32-bit float, 2 s past the last clip, the same
  bytes as the stem).
- `stems_tests.rs`: `stems_write_one_file_per_track_in_a_folder` (four files in order, 32-bit float, the right
  length, progress in order with each stem's name, a second folder), `stems_leave_out_tracks_muted_in_the_song`,
  `stems_of_a_song_with_plugins_are_refused_in_words` (the probe plugin; nothing made; the check's words the same),
  `one_export_at_a_time` (stems while exporting and the reverse; quitting stops stems and leaves no folder;
  another session's window refused with the slot left free).
- Suite `st-stems` (kind `export`, 25 checks, the API only for now): the check says none, the folder, an export
  refused meanwhile, the four names in order, each stem the same file `m2 bounce --track` writes on a copy of the
  song (the `fmt ` and `data` chunks, then every byte); Hats muted in the song leaves three, in `first-song stems 2`;
  Keys playing a plugin: the check's words, the refusal in the same words, no folder.
- The renders: 10 of 10 (`e2e/baseline.sh --tree`, then `shasum -a 256 -c e2e/renders.sha256`).

**Red first**

- `render.rs` with `write_float` writing 24-bit as before: both new tests fail (`render.rs:1033`: "kick: the stem
  and the bounce differ"; `render.rs:1054`: the peak).
- `stems.rs` rendering no stem, and no plugin refusal: all four fail, e.g. `stems_tests.rs:67`: `left: (Bool(true),
  Number(0), Bool(true)), right: (Bool(true), Number(4), Bool(true))`.
- `bounce.rs` against the 24-bit `write_float`: `bounce.rs:56`, "the stem and the bounce differ".
- `st-stems` on the before page and server: `FAIL  first-song's stems can be made: {"error":"no such API"}`,
  `FAIL  they go in a new folder in Exports: {"error":"no such API"}`.

**Guards**

- None: each new test fails against its stub.

**What's left**

- The Export dialog (C3b). Until it lands, a stems `done` event shows the old export toast.
- Stems of a song that plays plugins (Q40: its render would need the live instances lent to each track's graph).
- A song whose render makes a NaN: the stems carry the silenced samples, where `m2 bounce` stops with an error (D15).
  No fixture does.

**For gaps.md:** #36: stems: each track as its own WAV file (32-bit float, `m2 bounce --track`'s bits), into a new
folder in Exports; not yet for a song that plays plugins.

**Contract:** a stem is the track's own output before its mixer channel, rendered as `m2 bounce <song> x --track <key>`
renders it, the same bytes (RENDER-3). Stems and Export Audio share one slot: one at a time.

### C3b. The Export dialog

**In short**

| What | Now |
|---|---|
| ⇧⌘E, the Export button, File ▸ Export… | A dialog, "Export", with three choices |
| Song | The whole song as one WAV file, as Export did before. It is chosen, so Return does what Export did |
| Stems | Each track as its own WAV file (C3a). The Export button says "Exporting stems: Hats (2 of 4)". Then: "Exported 4 stems to first-song stems", with Show in Finder |
| MIDI | The song's notes as a MIDI file (C1's song export) |
| A song that plays plugins | Stems is greyed, and says why |
| Keys | Return goes on, Esc closes, Tab goes round, arrows move the choice. On a PC: Ctrl+Shift+E |

**What changed**

- `web/src/components/ExportDialog.tsx` (new, mounted in `App.tsx` after `<RenameSongDialog />`; ExportAsk lives in
  the top bar's `OverlayPill`, so "after `<ExportAsk />`" had no place in `App.tsx`): the frame of every page dialog
  (440 px, `animate-dialog`, `useModalKeys`). Three rows, each a native radio in a label (a click, Space and the arrow
  keys choose; Return from one goes on), with D34's words. Song is chosen and Export… has the focus. Stems asks
  `/api/graph/stems/check` as it opens and is greyed with the words. A double-click on a row goes on.
- `web/src/lib/exporting.ts` (C's): `exportSong()` opens the dialog (or says an export runs, as before);
  `exportChosen(choice)`: Song is `exportAudio()` (what `exportSong` did: ExportAsk's question while mutes or solos
  change what you hear, then the save panel in the app); Stems is `exportStems()` (no question, no panel, D23); MIDI
  is `exportMidi()` (C1). `useExport` gains `stem` (name, number, how many) from the stems' progress events;
  `onExportEvent` says a stems `done` as "Exported 4 stems to first-song stems" (with Show in Finder in the app; the
  folder after it in a browser) and an error as "Couldn't export the stems of “first-song”: …".
- `web/src/components/TopBar.tsx` (one hunk in `ExportButton`, for the integrator: TopBar is B's and D's
  otherwise): the tooltip says "Export the song, its stems or its notes (⇧⌘E)", and while stems render "Exporting
  stems: Hats (2 of 4)".
- `web/src/components/Shortcuts.tsx`: File's row "Export audio" reworded "Export" (in place).
- `apps/music2-desktop/src/menu.rs`: `export`'s title "Export…"; the test `file_export_opens_the_export_dialog` at
  the test module's end.

**Tests**

- `menu.rs`: `file_export_opens_the_export_dialog` (the title and ⇧⌘E; in File on both menu bars).
- `st-stems` grows to 49: the dialog's title, rows and words, Song chosen, Export… focused, the lint, Tab round its
  five controls, Esc; Return exports the song (`first-song.wav`); Stems from a click then Return, the button's
  "Exporting stems: …(n of 4)" and the toast "Exported 4 stems to first-song stems 3, in ~/Music/music2/Exports.";
  MIDI writes `first-song.mid` (five tracks) with "Saved the song as a MIDI file: first-song.mid, in
  ~/Music/music2/Exports."; a song that plays plugins: Stems greyed with Q40's words, Export… still on, the lint;
  a PC page's tooltip says Ctrl+Shift+E, which opens it.
- Moved (their checks' values only, D25; no count changes): `export` (Return in the dialog after each click and
  ⇧⌘E; the tooltip's words), `fx-fixes` (Return after four clicks; the click while another song exports still says
  so with no dialog), `s8-mute` (Return before each question: the question comes after the dialog, D23),
  `pc-keys` (the sheet's row "Export"), `live3b` (Return after its export click; its plugin part runs only with
  `CLAP_PATH`). `taste` passes unchanged: its "an export under way" state now lints the open dialog (taste.mjs is
  nobody's to edit, so it doesn't press Return).

Checked in the Linux app on the invisible screen `:97` (`app-shot.sh`): Ctrl+Shift+E opens the dialog (Song chosen,
Export… focused), and the File menu reads "Export…  Shift+Ctrl+E".

**Red first**

- `menu.rs` with the old title: `left: Some(("Export Audio…", …)), right: Some(("Export…", …))`.
- `st-stems` on the before page (`DIST=` the `85b069a` page, this server): the API part's 25 pass, then
  `FAIL  ⇧⌘E opens the Export dialog`.

**Guards**

- None.

**What's left**

- Q30, at the End: Export… greyed with "Stop recording first." while recording.
- `st-tips` places for the dialog (the End joins them; tiplib is P0's).
- `taste`'s "an export under way" state shows the dialog now, not the ring: the ring is still checked by `export`
  and `fx-fixes`. The End may teach `taste.mjs` to press Return there.

**For gaps.md:** #36: File ▸ Export… (⇧⌘E) offers Song, Stems and MIDI.

**Contract:** Export (⇧⌘E) is a dialog: Song (the WAV, with the mute question first), Stems, MIDI. Only Song asks the
question.

## Lane D

Not merged yet.

## Merge

The lead joins the lanes.

## End

The lead's last checks, docs and pictures.
