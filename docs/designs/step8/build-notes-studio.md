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

- `smf_tests.rs` (13): `reads_the_spec_example` (SMF 1.0's own example, as format 0 and as format 1: its programs,
  tempo, meter and four notes), `reads_format_0_and_1`, `running_status_and_sysex`, `velocity_0_ends_a_note`,
  `a_note_without_an_off_ends_at_the_next_on`, `tempo_and_meter_events`, `an_smpte_division_is_refused_in_words`,
  `format_2_is_refused`, `a_cut_short_file_says_where`, `too_big_or_too_many_notes_is_refused_in_words`,
  `writes_what_it_reads` (both spec files, and the bytes of a written track), `a_mangled_file_never_panics`
  (every prefix of the spec file, then 2,000 seeded files with bytes flipped).
- `midifile_tests.rs` (8): `the_song_has_a_tempo_track_and_a_track_per_lane` (first-song: Kick and Hats on channel
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

## Lane D

(Lane D's.)

## Merge

(The lead's.)

## End

(The lead's.)
