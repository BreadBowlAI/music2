# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

(The lead's.)

## Lane A

(Lane A's.)

## Lane B

(Lane B's.)

## Lane C

MIDI files in and out, and stems from the Export dialog (gaps #36). Branch `studio-c`, worktree `wt/st-c`, from
`studio` at `85b069a` (P0 hadn't landed: lane C made its own module stubs, as the plan's §2.1 names them, and
the merge joins them).

**For 8a's merge:** lane C changes nothing in `core/runtime`, `core/play` or `core/os`.

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

(Lane D's.)

## Merge

(The lead's.)

## End

(The lead's.)
