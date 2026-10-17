# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

P0 never reached `studio` while the lanes ran. Lane A built the P0 pieces it needed to the plan's spec
(`tracks::add_tracks` with `Play` and `NewTrack`, the drop layer, the app's `open_file`), and each lane made its own
seams (`mod` and `.merge` lines). The merge kept one copy of each: see Lane A's table and the Merge section.

## Lane A

Audio in the app: import, audio tracks, waveforms, and editing audio clips. Branch `studio-a`: A1 `b7cd94a`, A2
`56b064d` (the menu), A3 `0d3a991`, A4 `b15f974`, A2's drops and audio tracks `d4b88ab`, the app's open dialog
`f620cd0`, the review's fixes `6bf0b1c`; each green.

**In short, for the owner**

| You can | How |
|---|---|
| Bring in a WAV file | File ▸ Import Audio… (⇧⌘I), or drag it from the Finder onto a track, or below the tracks |
| Make an audio track | + Track ▸ Audio track (type "audio") |
| See the sound | Every audio clip shows its waveform |
| Shape a clip | Trim either edge (it stops where the file ends), drag the top corners for fades, Volume… and Reverse in its menu |
| Take it back | One Undo for each step: "Undo importing vox.wav", "Undo Vox fade in" |
| From the terminal | `m2 import <song> <file.wav>… [--track ID] [--at BAR]` |

**For the merge with P0 and the other lanes:** P0 never reached `studio` while lane A ran, so lane A made its own
seams, and built the P0 pieces it needed to the plan's spec (each listed in its slice):

| Where | Lane A's | At the merge |
|---|---|---|
| `crates/server/src/lib.rs` | `mod routes_audio;` and `mod peaks;` after `mod routes_leases;` | Beside P0's and lane B's lines; take lane A's whole files over P0's empty stubs |
| `crates/server/src/routes.rs` | `.merge(crate::routes_audio::router(…))` after `routes_leases`' | Beside the others |
| `crates/server/src/graph.rs` | `mod import;` after `mod tempo_lease;` | Beside lane B's `pub mod record;` and lane C's |
| `crates/editor/src/lib.rs` | `pub mod import;` after `pub mod rekey;` | Beside lane C's and D's |
| `crates/editor/src/render.rs` | `part_path`, `sweep`, `name_new` `pub(crate)` | The same three words as P0's (lane C owns the file) |
| `crates/editor/src/tracks.rs` | P0.3: `add_tracks`, `Play`, `NewTrack` | Keep one; lane C's C2b can use it |
| `web/src/lib/drop.ts`, `components/DropLayer.tsx`, `App.tsx`' `<DropLayer />` | P0.4 | Keep one; lane C registers `.mid` with `onFiles` |
| `apps/music2-desktop/src/actions.rs` | P0.5: `open_file` | Keep one |
| `web/src/components/ClipMenu.tsx` | `MenuItem.checked` (a check, and every row its room) | Lane B's Record menu may add checks too: keep one field |
| `web/src/App.tsx` | `import_audio` in `chord()` and its `case`, `DropLayer` and `importAudio` imports | Join with B, C and D's cases |
| `apps/music2-desktop/src/menu.rs` | `import_audio` after `save_as`; File: Save As, a line, Import Audio…, a line, Export | Lane C's Import MIDI… goes after Import Audio…, before the line |
| `crates/server/src/graph/lanes.rs` | `Gesture::Audio` in `named`'s pattern | — |
| `e2e/run.mjs`, `e2e/README.md` | `st-audio` after `s8-leases`, 90 checks | `tips()` once `tiplib.mjs` (lane D) is there |
| `e2e/s8-tracks.mjs` | the list's groups and rows end with Audio (values only) | — |

**For 8a's merge:** nothing: lane A changed nothing in `core/runtime`, `core/play`, `core/os` or `crates/gates`.

### A1. Waveforms

**In short**

| What | Now |
|---|---|
| An audio clip | Shows its sound as a waveform, in the track's colour, under the file's name |
| A quiet file | A flat line |
| A clip played backwards | Its waveform is drawn backwards too |
| A clip made quieter | Its label says so ("tone.wav · −6 dB"), and the waveform is smaller |
| A file that is gone | The clip says the file's name, as before |
| Speed | The waveform is made once per file and kept. A song opened again draws at once |

**What changed**

- `crates/server/src/peaks.rs` (new): `Peaks` (min and max per 256 frames per channel, as `i8`, Q9), `Store::of(path)`:
  the file's hash (`HashCache`), then the last 32 in memory, then `<cache>/peaks/<sha256>.m2pk`, else one decode with
  `load_wav`, written whole through a part file and a rename. `Store::put(sha, &Asset)` keeps peaks from a decode
  already made (A2's import, D31). A cache file that doesn't read is made again. Tests keep peaks in memory only.
- `crates/server/src/routes_audio.rs` (new): `GET /api/graph/audio/peaks?session=&asset=` answers the bytes
  (`application/octet-stream`), made on `spawn_blocking`. A read: a read-only window may ask. Its own write list
  (`WRITES_AUDIO`, empty so far) and 423 middleware, as `routes_view.rs` has.
- `crates/server/src/graph/import.rs` (new): `Graph::asset_file(token, name)`: the asset's file in the open song, from
  the control plane only, never the session.
- `crates/server/src/{lib.rs, graph.rs, routes.rs}`: the `mod` lines (`routes_audio`, `peaks`, `import`) and the
  router's `.merge` line, where P0's stubs put them.
- `web/src/lib/peaks.ts` (new): `usePeaks(clip)` (one request per file and pin, kept while the page lives; one that
  failed is asked again when the song changes), `readPeaks`, and the tempo map's `secondsAt`, `beatAt`,
  `clipSeconds` (as `TempoMap`).
- `web/src/components/Wave.tsx` (new): the clip's name strip (14 px, `text-xs`, `ink`, taste §2.8) and its waveform:
  canvas tiles of 1,024 px, each drawn once it is on screen; each pixel's song seconds through the tempo map, then
  `offset` plus the seconds in (backwards when reversed, times the pitch's rate); the loudest sample of every channel
  there, mirrored about the middle, times the volume and the fades (clipped at the body's edge); each fade a 1 px
  `fg-muted` curve. A missing file: today's icon and name.
- `web/src/components/SongTimeline.tsx` (A's block): the audio branch of `inside()` draws `<Wave />`.

**Tests**

- `crates/server/src/peaks_tests.rs`: `peaks_are_min_and_max_per_256_frames` (a ramp and a sine, a short last bucket,
  over full scale), `a_mono_file_has_one_channel`, `peaks_are_made_once_and_kept_by_content` (a second ask decodes
  nothing; a copy under another name is the same entry; a new store reads the cache folder), `a_cache_file_that_doesnt_read_is_made_again`
  (cut short, another magic, junk), `a_file_that_doesnt_decode_says_why`, `the_cache_is_under_home` (PLAT-7),
  `peaks_read_back`.
- `crates/server/src/routes_audio_tests.rs`: `peaks_of_an_asset` (the bytes; `../song.ts`, another session and a
  missing file refused in words), `peaks_never_wait_for_the_actor` (**guard**: the actor held as a compile holds it).
- `e2e/st-audio.mjs` part 1 (kind `fix`, 16 checks): a Tone track with six clips; each with a file draws a canvas;
  a full-scale tone fills the body all across in the track's ink; its name in the strip; silence is a flat line;
  a swell grows, and its reversed clip is its mirror, column by column; −6 dB draws half as tall and says
  "tone.wav · −6 dB"; a missing file keeps its icon and name; four zooms ask for no peaks again (one request per
  file); lint of the lane.

**Red first**

- Rust (`Peaks::of_asset` empty and `Store::find` finding nothing): 5 of 9 fail, e.g.
  `peaks_are_min_and_max_per_256_frames`: `left: (0, 0, 0, 256) right: (2, 48000, 1000, 256)`;
  `a_mono_file_has_one_channel`: `left: (0, 0, 0) right: (1, 600, 6)`; `peaks_never_wait_for_the_actor` panicked
  reading the answer.
- `st-audio` on `a-dist-before` and the 85b069a server: `FAIL each clip with a file draws a canvas: 0`,
  `FAIL the tone's middle row has ink all across: 0 columns`, then it stops: no canvas to read.

**Checks**: `cargo clippy -p m2-server --all-targets -D warnings`, `cargo test -p m2-server` (311 + 1), the web build,
`st-audio` 16/16, `taste` 39/39, `s8-clips` 94/94.

**Left**: `tiplib.mjs`' `tips()` in st-audio (P0's file, not on `studio` yet); the clip's tooltip names the fade
handles once A3 adds them.

**For gaps.md**: #18: audio clips show their waveform (peaks cached by content in `<cache>/peaks`).

**Contract**: FMT-16 and RENDER-30: peaks (min and max per 256 frames, `i8`) are kept by content in
`<cache>/peaks/<sha256>.m2pk`, made from one decode, never on the actor or the audio thread; a cache file that doesn't
read is made again.
### A2, first part. Bring audio in from the menu

**In short**

| What | Now |
|---|---|
| File ▸ Import Audio… (⇧⌘I, Ctrl+Shift+I on a PC) | Pick one or more WAV files. They land on the selected audio track, at the bar where the playhead is, one after another, and are selected |
| Undo | One Undo takes the whole import back: "Undo importing vox.wav", "Undo importing 3 files". The files stay in the song's `assets/` folder |
| A file that isn't a WAV | Refused in words: "song.mp3 isn't a WAV file. music2 reads WAV files. Save it as a WAV first: most audio apps can (Export, then WAV)." Nothing is added |
| Other refusals | Too long (over 15 minutes), no sound in it, a rate music2 can't take, a read-only window. Each says why |
| The same file twice | The copy already in `assets/` is used again. A different file with a taken name gets a number: `vox 2.wav` |
| A file with 6 channels | It plays the first two, and a toast says so |

Not yet: drops, new audio tracks and + Track ▸ Audio track. They need P0's drop layer and `tracks::add_tracks`, which
are not on `studio` yet (see Left).

**What changed**

- `core/song/src/path.rs` (A's block): `AssetName::for_import(file_name)` (Q2: NFC, control characters dropped, `/`
  `\` `:` made `-`, leading dots and spaces dropped, the extension lowercased, at most 120 bytes cut on a character,
  else `audio.wav`) and `untaken_folded(names)` (a taken name compared in any capitals and Unicode form, D2).
- `crates/editor/src/import.rs` (new): `audio(dir, Source::Path | Source::Bytes)` checks the header first (`RIFF`/
  `WAVE`, readable, 8 to 384 kHz, has frames, at most 15 minutes: D31), then uses the same bytes already in `assets/`
  (size, then hash) or copies through a part file made create-only and locked at once, pushed, decoded once with
  `load_wav`, named create-only (the next number when a name is taken meanwhile), its folder synced (Q3, D2). A failure
  removes the part. `take_in(s, files)` (D1) takes the new files into the session before a Tx names them;
  `place(s, files, Target)` puts their clips onto an audio lane one after another as one `Gesture::Set` (Q5's length:
  the file at its own speed under the tempo there). `Target::{Lane, Track, Below}` is the drop layer's shape.
- `crates/editor/src/render.rs`: `part_path`, `sweep` and `name_new` are `pub(crate)` (P0's three words, made here too:
  the same change on both sides merges).
- `crates/editor/src/clips.rs`: a double-click on an audio lane says "An audio clip plays a sound file. Drop a WAV
  file here, or use File ▸ Import Audio…" (D21).
- `crates/server/src/graph/import.rs`: `Graph::import_audio(token, sources, target)`: checks what it can from the
  control plane before copying (D16: the song, read only, the lane is audio, the code compiles); copies under one
  mutex per server (D2); keeps the peaks of the import's own decode (D31); then one actor job places the clips and the
  server names the step "importing vox.wav" / "importing 3 files" (Q16).
- `crates/server/src/routes_audio.rs`: `POST /api/graph/audio/import`: JSON `{session, paths, to}`, or the files'
  bytes back to back with `?session=&files=[{name,size}]&to=` (one request, so one undo step for several files). Body
  limit 1 GiB on this route only. A write: 423 read only.
- `apps/music2-desktop/src/menu.rs`: File ▸ Import Audio… (`CmdOrCtrl+Shift+I`) after Save As and a line.
- `web/src/lib/audioImport.ts` (new): `importAudio()` (the app's `open_file` when the shell has it, else the page's
  file input), `menuTarget()` (Q7), `bringIn(files, to)` (paths or bytes; afterwards the clips selected, a toast only
  for a track added or a file changed on the way in; a refusal toasts "Nothing was added to the song: …").
- `web/src/App.tsx`: `case "import_audio"`, and ⇧⌘I in a browser's `chord()` (D36). `Shortcuts.tsx`: File "Import
  audio".

**Tests**

- `core/song`: `for_import_names_are_asset_names` (12 awkward names, and a long one cut on a character),
  `an_import_name_is_free_in_any_capitals`.
- `crates/editor/src/import_tests.rs`: `an_import_copies_the_file_create_only`, `a_taken_name_gets_a_number`,
  `the_same_bytes_use_the_asset_there` (in a folder too), `a_name_is_made_safe_and_nfc`,
  `a_file_that_isnt_wav_is_refused_in_words` (by header, not name; no sound; a 4 kHz file),
  `a_wav_that_doesnt_decode_leaves_nothing`, `a_file_over_15_minutes_is_refused` (by its header, before any copy),
  `extra_channels_are_counted`, `a_part_being_copied_is_never_swept` (a sweep mid-copy; and an old unlocked part is
  swept), `two_imports_of_one_name_both_land` (and a name taken mid-copy), `a_drop_on_an_audio_lane_is_one_clips_tx`
  (one after another, canonical, one undo, not onto a notes lane), `an_import_commits_a_revision_that_plays_the_asset`
  (A.2's replacement for `the_asset_is_whole_before_the_tx_names_it`), `the_clip_is_the_files_length_at_the_tempo_there`
  (over a tempo change), `a_file_name_makes_a_track_name`.
- `crates/server/src/graph/import_tests.rs`: `an_imported_clip_plays_its_file` (a float WAV whose samples are whole
  24-bit steps: the render right after the import has each one at the drop beat's frame), `a_path_and_an_upload_import_alike`,
  `a_read_only_window_refuses_an_import`, `a_refused_import_adds_nothing`, `the_peaks_are_ready_when_it_answers`,
  `one_undo_takes_an_import_back`.
- `crates/server/src/routes_audio_tests.rs`: `an_upload_and_paths_import_through_the_route`, `a_read_only_window_cant_import`.
- `apps/music2-desktop`: `import_audio_is_shift_command_i_in_file`.
- `e2e/st-audio.mjs` part 2 (33 checks in all now): ⇧⌘I through the file chooser lands two files on Tone at bar 5's
  line, one after another, their lengths at 124 BPM, each with its pin and id; selected; "Undo importing 2 files";
  `assets/` holds exactly the files, byte for byte; the lane file canonical; their waveforms; lint; one ⌘Z takes both
  back and the files stay; an MP3 says Q1's words and adds nothing; a PC page's Ctrl+Shift+I.

**Red first**

- `take_in` a no-op: `an_import_commits_a_revision_that_plays_the_asset`: `left: None right: Some(Hash(…))`;
  `an_imported_clip_plays_its_file`: `frame 0 of the file left: (0, 0) right: (-1000000, -1000000)`: the clip played
  silence.
- `st-audio` on `a-dist-before` and the 85b069a server: `FAIL Meta+Shift+KeyI opens a chooser that takes several
  files`, `FAIL ⇧⌘I brought two files onto Tone: 0`, `FAIL one undo step, named: Undo (⌘Z)`.
- The rest are new functions; their tests fail against stubs that return nothing.

**Checks**: clippy (`m2-song`, `m2-editor`, `m2-server`, `music2-desktop`) and the mac lint of them; their tests
(315 + 319 + 78 + 18); the web build; `st-audio` 33/33, `taste` 39/39, `pc-keys` 37/37, `s8-clips` 94/94, `s8-tracks`
75/75.

**Left** (waiting on P0): drops (`drop.ts` and its hint words), `Target::Track` and `Target::Below` (a new audio
track per file, via `tracks::add_tracks` with `Play::Audio`), + Track ▸ Audio track and `POST
/api/graph/audio/track`, the menu's new track when the selected one has no audio lane (it says so now), the app's
`open_file` (the page's file input stands in), `tips()` in st-audio.

**For gaps.md**: #18: audio files come in from File ▸ Import Audio… (WAV only; copied into `assets/`, the same bytes
used again, a taken name numbered); one undo takes an import back and the file stays (cleaning unused audio is left).

**Contract**: FMT-14 *Today*: importing copies create-only through a locked part file, pushed and decoded before it
is named, a taken name numbered (compared in any capitals), the same bytes used again. WRITE-1 *Today*: an imported
file is pushed and its folder synced, then taken into the session, before the Tx that names it. WRITE-10: import audio
(one `Gesture::Set`, named "importing …").
### A3. Edit audio clips

**In short**

| What | Now |
|---|---|
| Move, copy, split, duplicate, delete | As with notes clips. Each is one Undo. You hear the change when you let go |
| Trim the left edge | The clip starts later in its file. What is left plays where it did |
| Trim the right edge | It stops where the file ends. The bubble says "End of tone.wav" |
| Fades | Two small squares at the clip's top corners (on hover, or when it is selected). Drag one in for a fade. The bubble says "Fade in 0.40 s". Double-click a square: no fade |
| Volume… | In the clip's menu. Type a number of dB, or use ↑ ↓ (0.5 dB) and Shift with them (3 dB). Return sets it. The label says "tone.wav · −6 dB" and the waveform gets smaller |
| Reverse | In the clip's menu, with a check while it is on. The waveform turns round. A reversed clip can be trimmed and split too now |
| Remove fades | In the clip's menu. Greyed when the clip has none |
| Undo names | "Tone fade in", "Tone clip volume", "reversing the Tone clip", "removing the Tone clip's fades" |

**What changed**

- `crates/editor/src/clips.rs`: `Gesture::Audio { clip, gain_db?, fade_in?, fade_out?, reverse? }` (tag `audio`):
  only an audio clip; volume −48 to +12 dB; fades 0 or more and together at most the clip's seconds under the tempo
  map; one Tx, `Confined { lanes }`, as every gesture. Resize of an audio clip goes through `audio_span`: forwards the
  left edge moves the offset, backwards the right edge does (a reversed clip plays its span from its end); the file
  stops each edge (D3: forwards the left edge at its start and the right edge at its end; backwards the right edge at
  its start and the left edge at its end; 1 ms of slack, the length from the header by `asset_seconds`, no decode);
  only the edge that moves a limit is held to it. A reversed clip splits: the first part starts later in the asset,
  the second keeps the offset. `audio_edge` now refuses only a pitched clip's offset edge (as before) and a reversed
  clip with warp markers. `pinned` names the audio gesture's clip.
- `crates/server/src/graph/lanes.rs`: `named` gives the audio gesture's lane (one pattern, so a lease names it).
- `web/src/lib/clips.ts`: the `audio` gesture. `web/src/lib/peaks.ts`: `fileSeconds`, `edgeLimits` (each edge's limit
  in beats and its words), `respanned` (an audio clip's offset as the server moves it).
- `web/src/components/Wave.tsx`: the fade handles (7 px, top corners, shown on hover or when selected, on clips 24 px
  wide or more: D33; their own drag, never the clip's; the fade drawn as it is dragged; one gesture on release; Esc
  puts it back; a double-click takes it out), the bubble, `sendAudio`, `audioRows` (Volume…, Reverse with its check,
  Remove fades) and `VolumePopover` ("Clip volume", D20).
- `web/src/components/ClipMenu.tsx`: `MenuItem.checked`: a row that is on or off (`menuitemcheckbox`), and every row
  of such a menu gets the check's room so the words line up.
- `web/src/components/SongTimeline.tsx` (A's blocks): `reshaped` moves an audio clip's offset as the server will (its
  waveform stays put while an edge is dragged); an audio clip's edges stop at its file with the bubble's words; the
  audio rows after Split; `<VolumePopover />` beside Repeat's; the clip's tooltip names the fade corners and says its
  volume; `inside()` gives `Wave` what it needs to edit.

**Tests**

- `clips_tests.rs`: `fades_gain_and_reverse_are_one_tx_each` (one revision each, nothing else changes, four undos take
  them back, Remove fades), `fades_never_pass_the_clip`, `gain_stays_in_its_range`, `only_an_audio_clip_takes_audio_settings`,
  `a_reversed_clip_trims_from_either_edge_and_plays_the_same_samples` (the render's frames where both play are equal,
  to the bit, each way and back), `a_reversed_clip_splits_into_the_same_samples`, `the_edges_stop_at_the_file_both_ways`
  (four cases, the slack, a pitched clip's left edge). Changed: `an_audio_left_trim_moves_its_offset` and
  `a_split_audio_clip_plays_the_same_samples` no longer expect a reversed clip to refuse.
- `e2e/st-audio.mjs` part 3 (67 checks in all now): the right edge stops at the file's end ("End of tone.wav"), one
  undo step; a left trim moves the offset by a beat's seconds; the selected clip's 7 px fade handles; a fade dragged
  ("Fade in 0.50 s", written in seconds, "Undo Tone fade in"); the menu's rows after Split, Reverse unchecked, Remove
  fades on; Volume… (the field has the focus, ↓ steps 0.5 dB, −6 writes it, "Undo Tone clip volume", the label, the
  waveform half as tall); Remove fades; Reverse (written, named, the waveform mirrored, checked in the menu); a reversed
  clip's right edge trims; move, ⌘D, ⌫ each named; lint of the handles, the menu, the popover and the lane.

**Red first**

- The old rule (a reversed clip's edges and splits refused, no file limit): `a_reversed_clip_splits_into_the_same_samples`:
  `The audio clip's split can't move here: it is reversed.`; `a_reversed_clip_trims_from_either_edge…`: `…right edge
  can't move here: it is reversed`; `the_edges_stop_at_the_file_both_ways`: `Resize { … length: 8.01 } was accepted`.
- `st-audio` on `a-dist-before`: `FAIL the bubble says where the file ends: 10.3.1`, `FAIL the right edge stops at
  the file's end: 6`, `FAIL the selected clip shows its 7 px fade handles: 0`, `FAIL Volume…, Reverse and Remove fades
  come after Split: cut,copy,paste,duplicate,repeat,split,delete,loop`.

**Checks**: clippy and the mac lint (`m2-editor`, `m2-server`); their tests (322 + 319); the web build; `st-audio`
67/67, `taste` 39/39, `s8-clips` 94/94, `s8-leases` 55/55, `pc-keys` 37/37.

**Left**: the fades of several selected clips at once; Volume… on several clips; a pitched clip's left edge (pitch
isn't in the app); joining audio clips (Q15).

**For gaps.md**: #18: audio clips are edited in the app: trims (both ways, reversed too, stopping at the file),
fades, volume, reverse, remove fades.

**Contract**: WRITE-10 rows: audio clip settings (`Gesture::Audio`: volume, fades, direction), one Tx each. TL rows:
an audio clip's edges stop at its file, mirrored for a reversed clip.
### A4. `m2 import`

**In short**

| What | Now |
|---|---|
| `m2 import <song> <file.wav>… --track ID [--at BAR]` | Brings WAV files into the song from the terminal, as File ▸ Import Audio… does: onto a track's audio lane, one after another from that bar. One undo takes it back |
| While the app has the song open | It stops before copying anything and names the app, as every command that writes does |

**What changed**

- `apps/m2/src/main.rs` (A's arm and usage line): `import`: `session` (the lock), `--at` by `clips::position`
  (bar.beat.sixteenth, from 1), `--track` names a track with an audio lane, then `import::audio` for each file and one
  `import::place`. A track to make (no `--track`) waits for P0's `add_tracks`: it says to name one.
- `apps/m2/tests/lock.rs`: WRITE-18's list gains `import`.

**Tests**: `apps/m2/tests/import.rs`: `import_places_a_clip_and_takes_the_lock` (two files at bar 9 one after another,
the copy byte for byte, no `--track` and a text file refused in words, the lock held: refused, nothing copied).
`lock.rs`' list with `import`.

**Red first**: with the arm taken out, `m2 import` prints the usage: `panicked at apps/m2/tests/import.rs:50:5: m2:
build, check and render music2 projects`.

**Checks**: clippy and the mac lint of `m2`; `cargo test -p m2` (every test file green).

**Left**: `m2 import` onto a new audio track (with P0's `add_tracks`).

**For gaps.md**: #37's first item: `m2 import` (onto a track's audio lane). `m2 bounce --place` keeps its gap (D1: it
names the asset before the session takes it in): one line.

**Contract**: WRITE-18's list of writers gains `m2 import`.
### A2, second part. Drops and audio tracks (with P0's pieces built here)

**In short**

| What | Now |
|---|---|
| + Track ▸ Audio | The list's last group, "Audio": "Audio track · Plays recordings and loops: drop a WAV file on it". Typing "audio" finds it. "Added Audio", with Undo |
| Drag WAV files onto the window | Onto an audio track: there, at the bar. Onto another track: a new audio track right after it. Below the tracks: a new audio track for each file, at the end. Each named after its file ("Drum loop") |
| While dragging | A blue outline where the files land, and words: "Audio · bar 2", "New audio track · bar 3", "3 new audio tracks · bar 5". Another kind of file: "music2 opens WAV and MIDI files" |
| File ▸ Import Audio… with no audio track selected | A new audio track after the selected one (at the end with none) |
| The toast | Only when a track was added: "Added Drum loop, playing drum loop.wav", with Undo |

**For the merge with P0:** P0 wasn't on `studio` while lane A ran (nothing of it by the evening of Day 1), and
without its pieces nobody could make an audio track in the app. Lane A built them to the plan's P0 spec, each so
the integrator keeps one side:

| P0 piece | Built here | At the merge |
|---|---|---|
| P0.3 `tracks::add_tracks` (`Play`, `NewTrack`, `adds`, `add` as `add_tracks` with one track) | `crates/editor/src/tracks.rs`: one block replacing `adds_one`/`add` (`adds_one` kept as `adds` with one), plus `AUDIO_LANE`, `audio_call_text`, `joined_entries`, `with_fresh_ids`, `unique_label_among`; its four tests and one more in `tracks_tests.rs`; the audio form in `gate5_tests.rs` and `tests/forms/forms.ts` | If P0 built its own, keep P0's and point `import::place` and `Graph::add_audio_track` at it (same names, same shapes) |
| P0.4 the drop layer | `web/src/lib/drop.ts` (`onFiles(extensions, types, {hint, drop, where})`, `target(x, y)`, `listenForDrops`, `useDropHint`) and `components/DropLayer.tsx` (mounted after `<Toasts />`). The beat origin is the ruler's left edge, so `SongTimeline.tsx` needs no `data-beat-origin` | Keep one. Lane C registers `.mid` with `onFiles` (its `types` argument is new: the MIME types a browser says while it drags) |
| P0.5 `open_file` | Not built: the page's file input stands in, in the app too (WebKit shows the system's dialog for it) | `importAudio()` already asks `nativeAction("open_file")` first, so P0's action takes over by itself |
| P0.6 `applib`/`droplib` | Not built: `st-audio` has its own `wav()` and drops files as a browser does (a `DataTransfer` with `File`s) | The app page's Tauri drag events are checked by hand on the Mac (HANDOFF) until `applib` lands |

**What changed (lane A's own)**

- `crates/editor/src/import.rs`: `place` with `Target::Track` and `Target::Below`: one `add_tracks` with a
  `NewTrack { play: Audio, label: "Lead vox" for lead vox.wav, clips: [its clip] }` per file, after the track or at
  the end; the clips made are read back for the page to select.
- `crates/server/src/graph/import.rs`: `Graph::add_audio_track(token, label, after)`, the step named "adding Audio".
  `routes_audio.rs`: `POST /api/graph/audio/track {session, label?, after?}` (a write: 423 read only).
- `web/src/lib/audioImport.ts`: `AUDIO_TRACK` (+ Track's Audio row) and `addAudioTrack`; the WAV kind registered with
  the drop layer: its hint words, `where` (the outline on the lane it really goes to: a notes lane's drop outlines a
  new track under it), and its drop; the import's toast has Undo when a track was added.
- `web/src/components/Tracks.tsx` (A's block): the Audio group last when adding (never in Change Instrument), its
  foot `tracks/audio.ts · lane.audio("clips")`.
- `web/src/components/SongTimeline.tsx`: an audio lane's tooltip says "Drop WAV files here, or use File ▸ Import Audio
  (⇧⌘I)".
- `e2e/s8-tracks.mjs`: the list's groups and rows end with Audio (the check's values; its count stays).

**Tests**

- `tracks_tests.rs`: `two_tracks_in_one_tx_are_one_undo_step`, `an_audio_track_plays_its_lane` (the render: the
  clip sounds, nothing else changes), `a_new_track_carries_its_clips_with_fresh_ids`,
  `an_arrangement_written_with_new_tracks_is_admitted` (one undo takes the tempo back too),
  `two_new_tracks_of_one_name_stay_apart`; every earlier `tracks_tests` test unchanged and green.
- `import_tests.rs`: `a_drop_below_the_tracks_adds_a_track_and_its_clip_in_one_step`,
  `a_drop_on_a_notes_track_adds_an_audio_track_after_it`.
- `routes_audio_tests.rs`: `an_audio_track_through_the_route`.
- `st-audio` part 4 (90 checks in all): "audio" finds the Audio group, its foot, lint; Enter adds Audio (its file, its
  empty lane, "Added Audio", "Undo adding Audio"); a drop below the tracks ("New audio track · bar 3", a dashed
  outline) makes "Drum loop" at bar 3 with its waveform, "Undo importing drum loop.wav"; a drop on Kick ("New audio
  track · bar 2", outlined under Kick) makes an audio track right after Kick; two files onto the Audio lane ("Audio ·
  bar 2", a solid outline, linted) land one after another; the hint goes after a drop; an MP3 says "music2 opens WAV
  and MIDI files" and writes nothing; an instrument's drag shows no file hint (D12); lint.

**Red first**

- `add_tracks` as one `add` per track: `two_tracks_in_one_tx_are_one_undo_step`: `one revision left: 3 right: 2`.
- `st-audio` on `a-dist-before`: `FAIL typing "audio" finds the Audio group: []`, then no foot line to read.

**Checks**: clippy (`m2-editor`, `m2-server`, `m2`) and the mac lint; tests (329 + 320; one timing test,
`midi_never_waits_for_a_compile`, failed once under the machine's load and passed three times alone: unchanged code);
the web build (with the forms' type check); `st-audio` 90/90, `taste` 39/39, `s8-tracks` 75/75, `pl-tracks` 20/20,
`pc-keys` 37/37, `s8-clips` 94/94, `pl-sounds` 45/45, `s8-leases` 55/55, `nx-effects` 47/47.

**Left**: the app's Tauri drop checked by a suite (P0.6's app page), `open_file` (P0.5), `tips()` in st-audio;
`m2 import` onto a new track (it can use `add_tracks` now: a small follow-up).

**For gaps.md**: #18: drops (onto a track, a new track, below the tracks) and + Track ▸ Audio track.

**Contract**: WRITE-10 rows: several tracks in one Tx (`add_tracks`, with an arrangement written whole); an audio
track (`lane.audio("clips")`, no instrument import). taste.md §3.6: + Track's last group is Audio. The drop hint.
### A2, third part. The app's open dialog, and `m2 import` onto new tracks

**In short**

| What | Now |
|---|---|
| ⇧⌘I in the app | The system's open dialog, "Import Audio", showing WAV files; several can be picked. Checked on Linux on `:97`: a WAV picked lands on a new "Swell" track after the selected Kick, with its waveform, "Undo importing swell.wav" |
| `m2 import <song> <file.wav>…` with no `--track` | Each file on a new audio track named after it, at the end |

**What changed**

- `apps/music2-desktop/src/actions.rs` (P0.5, built here to its spec): `open_file`: arg `{"title", "filters": [{"name",
  "extensions"}], "multiple"}` (read by hand: the desktop has no `serde`, and `Cargo.lock` stays), answers a JSON
  list of paths or null, one dialog at a time (`BUSY`). Before it, an unknown action showed an error dialog in the
  app ("unknown action open_file"), seen on `:97`.
- `apps/m2/src/main.rs`: `import` without `--track` places each file on a new audio track (`Target::Below`).

**Tests**: `actions.rs`: `the_open_dialog_takes_its_ask_from_the_page`. `apps/m2/tests/import.rs` gains a new track
from the terminal and a track with no audio lane refused. The app on `:97`: File ▸ Import Audio… is Shift+Ctrl+I after
Save As and a line; Ctrl+Shift+I opens the dialog; a file picked lands (pictures in `scratch/a-app1`, `a-app3`, `a-app8`).

**Red first**: the app on `:97` before: Ctrl+Shift+I showed "unknown action open_file" (`scratch/a-app2`).

**Checks**: clippy and the mac lint of `music2-desktop` and `m2`; `cargo test -p music2-desktop` (19 and the main-thread
test), `m2`'s `import` and `lock`.

**For the merge with P0:** P0.5's `open_file` is this one; if P0 made its own, keep one (same arg, same answer).
### A review of lane A, and its fixes

A code review of `85b069a..f620cd0` (medium) found ten things; each is fixed here, or said why not.

| Found | Now |
|---|---|
| A trim or a split never shortened the fades, and `Gesture::Audio` held them to the clip on every change: a volume or a reverse failed on a clip trimmed shorter than its fades | `fit_fades` after every trim and split (the fade in first, then what is left); the gesture holds fades to the clip only when it sets one. Test `a_trim_keeps_the_fades_inside_the_clip` (red without: `A clip's fades together…`, the volume refused) |
| A warped, pitched clip's left edge moved, where before A3 it refused | Refused again (`audio_edge` for a warped clip's left edge). Test `a_warped_pitched_clips_left_edge_still_refuses` (red without: the resize was accepted) |
| A track an import made kept a gone track's mute or solo of the same key | `forget_overlays` for each track an import adds, as + Track does. Test `a_track_an_import_makes_starts_heard` |
| A browser's drag of a WAV whose type it doesn't say (or says another way) was refused while dragging, so it couldn't be dropped | While dragging, only a type that is surely another kind (`audio/mpeg`) is refused; the name decides at the drop. `audio/x-wave` added |
| The clip's pin was the source's hash, read before the copy | The hash of the part file as written |
| An upload's sizes could overflow when added up, then slice out of range | A checked sum; one file's bytes are taken as they came, with no copy |
| One decode at a time for every file | One at a time per file (a lock per hash) |
| `edgeLimits` said a warped clip has no limits, but limited it | It doesn't limit a warped clip, as the server doesn't |
| `unique_label` and `unique_label_among` were two copies | One: `unique_label` calls the other |
| `routes_audio.rs`' 423 middleware copies `routes_view.rs`' | Kept: every route file has its own, by the plan's design |

**Checks**: clippy and the mac lint (`m2-editor`, `m2-server`); tests (331 + 321); the web build; `st-audio` 90/90,
`s8-clips` 94/94, `taste` 39/39, `s8-tracks` 75/75. Every browser suite at `d4b88ab` (`all`): 57 of 59 ok; `live3b`
PART (no `CLAP_PATH`, as always here); `attention` failed two toast checks once under the machine's load and passed
57/57 alone (it touches nothing of lane A's).

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

Plain-language help. Branch `studio-d`.

### D1. A tooltip on every control

**In short**

| What | Now |
|---|---|
| Hold the mouse over a button, knob, fader, tab or switch | A short line says what it does, and its key |
| A track's header | "Kick: click to show it in the dock below · drag to move it · right-click for more" |
| M and S | "Mute while you listen (M)", "Solo: hear only the soloed tracks (S)" |
| A knob in Device | How to use it comes first. Where it is in your code comes last |
| The key's Major and Minor | "Major: a bright, happy sound", "Minor: a darker, sadder sound" |
| A file in Code | What the file is: "song.ts: the song's main file. It brings the tracks together" |

**What changed**

- `e2e/tiplib.mjs` (new; P0 hadn't landed, so D made it, as the plan's §7.1 gives D the file after P0):
  `tips(sel, keys)` runs in the page. It asks for a tooltip on an icon button, a toggle (`aria-pressed`), a tab, a
  switch, a slider, a knob or fader (`[data-knob]`), a spin button, and every control `keys` names (selector → its
  key as `chord()` spells it, which the tooltip must hold). A text button that says its action and has no key needs
  none ("Done"), nor a segmented choice whose words name it ("Minor") (D19). Menu and list rows and the code
  editor's insides are left out (Q43). A tooltip is the control's own `title`, or an ancestor's that holds no other
  control; a knob's may sit round its slider. A tooltip that only repeats the control's words is listed too.
- Titles, attribute-only hunks (§7.2):
  - `SongTimeline.tsx`: the track header (`[data-module-tab]`, a `role=tab`) says what a click, a drag and a
    right-click do; the Song row says what it shows.
  - `NoteView.tsx`: Length's − and + ("One bar shorter", "One bar longer").
  - `CodeView.tsx`: each file's row says what the file is, in place of its path again.
  - `KeyField.tsx`: Major and Minor say how they sound.
  - Reworded (jargon or the wrong thing first): `MuteSolo.tsx` M ("Mute while you listen (M)") and S ("Solo: hear
    only the soloed tracks (S) · ⌥-click: solo only this track"); `NodeGraph.tsx` a Device knob: "Drag (⇧ for fine)
    · double-click to type a value · ⌥-click to reset · right-click to automate it", then "In your code:
    tracks/bass.ts, line 10 (written as 0.5)" (it started with `tracks/bass.ts:10`).
- `e2e/st-tips.mjs` (new, kind `fix`, 44 checks), its `run.mjs` row after `taste` and its README row.

**Tests.** `st-tips`: the lint's own check (planted controls: an icon with no tooltip, a toggle with none, a
tooltip that repeats "Export", a key not named; a "Done" button and a menu's row pass), then one check per place:
the start screen, New Song, the top bar, the track headers, M's and S's words, the timeline (as it opens, Bass's
automation shown, a clip picked, a marker), the Repeat popover, a marker's name field, the side browser, the dock's
header, a Device knob's words, the Device view of every track and the song, a knob's menu, the effect browser, Notes
and its tools, a note's menu, Mix, Code and its file list, Change track ID, Save As, Settings, the ⌥K sheet, + Track's
list, a toast, the Delete dialog, the key's popover, the mutes and solos popover, the export question, the status
popover with broken code, the keyboard piano's card, a MIDI keyboard's mark (injected), Rename Song (an app page's
menu), a PC page's top bar and dock header (Ctrl+Shift+E, Ctrl+Alt+B, Alt+1), and the taste lint. 44 of 44.
Moved (values only, no count changed): `pc-keys` 37, `taste` 39, `s8-mute` 152, `s5-knobs` 120, `code` 58,
`pl-notes` 102, `pl-mix` 47, `s8-mix` 80, `s8-tracks` 75, `pl-ids` 39, `s8-audition` 51, `nx-effects` 47, all ok.

**Red first** (`dist-before`, the server at `85b069a`): 35 of 44, 9 failed. 13 controls had no tooltip or one that
only repeated its words, on 5 kinds of place; and the words checks:

```
FAIL  the track headers: every control says what it does (5: no tooltip at <div[role=tab][data-module-tab=kick]> "Kick M S"; …)
FAIL  M and S say what they do: Mute (M) | Solo (S) · ⌥-click: solo only this track
FAIL  a knob's tooltip leads with how to use it, then where it is in the code: tracks/bass.ts:10  (written as 0.5) / Drag (⇧ for fine) · …
FAIL  Notes, its tools and keys: every control says what it does (2: no tooltip at <button> "One bar shorter"; no tooltip at <button> "One bar longer")
FAIL  the Code view and its file list: every control says what it does (1: a tooltip that repeats its words ("song.ts") at <button[data-file=song.ts]> "song.ts")
```

The first ten without one: the five track headers (Kick, Hats, Bass, Keys, Song), Length's − and +, song.ts's row
(its path again). Most of the app had its tooltips already (157 titles in `components/` at `85b069a`).

**Guards.** The lint's own first check (it can't be red on the old page: it tests `tips`, which is new).

**Left.** Lanes A, B and C's places join `st-tips` at the End (the Record button and its menu, the take, the
Export dialog, the tempo question, the guide, the volume popover, the drop hint), as the plan says. The read-only
window's banner and a hidden lane's "Show" line aren't walked (text buttons that say their action).

**For gaps.md:** #11: every control has a tooltip in plain words with its key, and `e2e/tiplib.mjs` checks it.

**Contract:** taste.md §5.3 gains: "Every icon button, toggle, tab, switch, slider, knob and fader has a tooltip in
plain words, with its key when it has one; a text button that says its action needs none. `e2e/tiplib.mjs`' `tips`
checks it, and every new suite runs it on what it adds."

### D2. Errors in plain words

**In short**

| What | Now |
|---|---|
| The code has a mistake | The status popover says what is wrong in a plain sentence, where, and how to fix it: "**Cutoff wants a frequency (Hz), but gets a level (dB).** tracks/bass.ts, line 12. Write it in Hz, such as hz(800)." |
| A mistake made through std (a level as a filter's cutoff) | It points at **your** line, not inside std. Show in code opens it there, marked |
| A misspelt name | "`pluk` isn't known in tracks/keys.ts. Did you mean pluck? Import it: `import { pluck } from "std/instruments"`." |
| A plugin, a sound file or a data file that's missing | Needs attention says it plainly, with what to do |
| The exact words a programmer wants | Under **Details**, as before |
| A mistake no rule knows | Its own words, as before |

**What changed**

- `core/ir/src/lib.rs`: `Diagnostic.inner: Option<Box<String>>`, serialized only when set (boxed: an unboxed
  `Option<String>` made `Result<_, Diagnostic>` 144 bytes, over clippy's `result_large_err`).
- `core/session/src/lib.rs`: `build_code` maps `m2_ir::check`'s errors through `at_project_call` (new fn after
  `build_code`, D's block): a `code.check` error with a node, placed outside the project, gets `src` = the innermost
  call in the project among the node's frames (`Module::chain(node.frame)`, a site with `in_project`), as
  `file:line:col` with the column in characters from the site's first byte; its old `src` becomes `inner`. An error
  already in the project, or whose frames reach no project call, keeps its place (Q46).
- `core/script/src/lib.rs`: `#[allow(clippy::result_large_err)]` on `described` and `describe_here` (two lines):
  `m2_ir::Diagnostic` is 128 bytes with `inner` even boxed, and clippy's limit is 128.
- `crates/editor/src/explain.rs` (new, `pub mod explain;` after `rekey` in `lib.rs`): `explain(code, message, &Song,
  &At) -> Option<Plain { words, fix }>`. `Song::of(session)`: the track files (from the folder: a song whose code
  never compiled has none in its revision), what packages export (each `src/*.ts`'s `export const|function|let`,
  then the catalog's components), the tracks' names. `At`: file, line, column, that line's text, the module. Rules
  (D2's table): a unit mismatch, an empty input (the check's "is not connected" and the SDK's "the input in is
  missing"), mono and stereo, a name not known (its import, or the nearest export a slip away: Levenshtein ≤ 2), a
  parser message (9 prefixes of oxc's: "There is a typo on line N of file"), "not a function", an import of nothing
  (the project's `import.outside-code`, and a package module's "file not found" or "Error resolving module": the
  track files, or the package's modules), `pkg.undeclared`, `key.duplicate`, `plugin.missing`, `asset.missing`,
  `lane.invalid`. The unit words in one table. Anything else: `None`.
- `crates/server/src/files.rs`: `Problem` gains `plain`, `fix`, `inner` (each serialized only when set);
  `problem()` fills `inner`, made relative; `diagnostics()` explains each draft error from its place and its line's
  text (the column in characters, before the UTF-16 conversion). `plain_problems(session, problems)` (new): the
  revision's problems as JSON, each with `plain` and `fix` when a rule knows it, from `Song::named` (the tracks'
  names and files: it never reads the packages' sources, as it runs at every publish that has a problem).
- `crates/server/src/graph/actor.rs`: one line where the published `problems` are made: through `plain_problems`.
- Web: `lib/plain.tsx` (new; `.tsx`, not the plan's `.ts`, for `Words`): `plain`, `fix`, `inner` on `Problem` and
  `plain`, `fix` on `Diagnostic` by `declare module "./graph"`; `placeOf` ("tracks/bass.ts, line 12"), `Words`
  (backticks set as code), `technical` (message · code · inner).
  - `Status.tsx`' `Compile`: an item in plain words is the sentence in 600, the place unless the sentence names the
    file, the fix, and its own Details (message · code · inner); others say "file, line N: message". Show in code
    sets `codeAt` to the problem's line, so the Code view opens there (it opened the track's file at the top).
  - `Attention.tsx`: the row's message is the plain words, the fix under it (`problem-fix`); the original message
    first in Details.
  - `CodeView.tsx`' problem list: each row the place (`file:line`, as code), the plain words and the fix; its
    Details under the row (a `div` round the button, as a `details` can't sit in a button).
  - `codeWorkspace.ts`: the editor's marker says the plain words and the fix, then the original.
  - `TrackHeader.tsx` (title only): a track's issue mark says the plain words, the original in brackets.
- `e2e/st-errors.mjs` (new, kind `fix`, 34 checks), its `run.mjs` row after `attention`, its README row.

**Tests.**
- `crates/editor/src/explain_tests.rs`, each on first-song with a file broken and opened as the app opens it:
  `a_unit_mistake_says_what_it_wants`, `an_empty_input` (the SDK's and the check's), `mono_and_stereo`,
  `a_name_not_known_proposes_its_import` (pluk → pluck; pluck and `lfo` unimported; nothing near),
  `a_typo_names_its_line` (two of oxc's messages), `a_missing_file_lists_the_tracks` (a track file; a std module),
  `not_a_function`, `an_undeclared_package`, `two_tracks_with_one_id`, `a_missing_plugin`, `a_missing_asset`,
  `a_lane_file_that_doesnt_read`, `an_unknown_message_keeps_its_words`. 13 of 13.
- `core/session`: `a_check_error_points_at_the_call_in_the_project` (`tracks/bass.ts:6:10`, and through the song's
  own component in `lib/dark.ts`: `lib/dark.ts:6:17`; `inner` in `std/src/filter.ts`).
- `crates/server/src/files.rs`: `diagnostics_carry_plain_words_and_the_technical_text` (and the JSON; one no rule
  knows has none; a revision's `asset.missing` through `plain_problems`); `diagnostics_say_where_in_the_project_each_
  problem_is` moved: its graph error is now at `tracks/lead.ts` line 3, `inner` in `std/src/`.
- `cargo test` of `m2-ir`, `m2-script`, `m2-session`, `m2-editor`, `m2-server`: all ok; their clippy clean.
- `st-errors` 34 of 34. Moved (values only, counts kept): `code` 58 and `graph` 19 (the banner's place is now
  "line N of file" or "file, line N"), `s4-problems` 18 (a row's text includes its Details: `lane.invalid`'s field is
  there now). Also ok: `s4-attn` 13, `attention` 57, `s6-write` 36, `s6-fixes` 19, `taste` 39, `st-tips` 44,
  `start` 24.

**Red first.** The rules a stub (`explain` returning `None`, `at_project_call` the identity): 12 of the 13 explain
tests fail, `no plain words for …` each (`an_unknown_message_keeps_its_words` is a guard), e.g.

```
no plain words for code.check: input `cutoff` expects control<hz>, got control<db>
no plain words for import.outside-code: `./tracks/bas` names no code file of the project (looked for …)
no plain words for asset.missing: data/vox/takes.json: `vox.wav` isn't in assets/, so the clip at beat 0 is silent
```

The session test: `left: Some("std/src/filter.ts:26:40") right: Some("tracks/bass.ts:6:10")`. The files test:
`left: (Some("std/src/filter.ts"), Some(26), Some(40)) right: (Some("tracks/lead.ts"), Some(4), Some(36))`.
`st-errors` on `dist-before` and the server at `85b069a`: 1 of 34, then it stops (no plain item to read):

```
FAIL  the server places it on your own line (std/src/filter.ts:26, line 12 wanted) in plain words: undefined undefined
FAIL  the banner says it plainly, where, and the fix: std/src/filter.ts:26:40 input `cutoff` expects control<hz>, got control<db>
```

**Guards.** `an_unknown_message_keeps_its_words`.

**Left.** A missing plugin is named by its track, not its own name (an uninstalled plugin's name isn't known: only
its id). "Not a function" names the thing only when QuickJS's column lands on the call. A unit mismatch names the
control by its port (`cutoff` → "Cutoff"), not the descriptor's label. `m2 check` prints the new place (your line)
but not the plain words.

**For gaps.md:** #11: the common code and data problems are in plain words with a fix, and a mistake made through std
points at your own line.

**Contract:** DIAG-1 *Today*: plain words are made in `crates/editor/src/explain.rs` from a problem's code and message
(core's words unchanged); `m2_ir::Diagnostic.inner` holds the place inside a package when a check error's `src` is
the call in the project that made the node (`core/session`'s `build_code`).

### D3. Make your first song

**In short**

| What | Now |
|---|---|
| Help ▸ Make Your First Song | A card at the top of the side browser: seven steps to a first song. The browser opens for it |
| The start screen | "New to making music? Make your first song": New Song on Empty, then the card |
| Each step | Ticks itself as you do it: drums, a beat, a bass, bass notes, a listen, a sound changed, an export |
| The step to do now | In bold, with what to press under it. Done steps get a check |
| All seven | "You made a song. It is in your Exports folder." (Show in Finder in the app) |
| The chevron, × | One line, "Your first song · 3 of 7"; × closes it, and its ticks stay |
| Next time | The card and its ticks are kept with the song, on this computer |

**What changed**

- `crates/server/src/graph/view_file.rs`: `KEYS` gains `"guide"` (D's line). Test (`view_file_tests.rs`, at its
  end): `the_guide_is_kept_per_song`.
- `apps/music2-desktop/src/menu.rs`: `ITEMS` `("guide", "Make Your First Song", None)` after `shortcuts`; Help
  starts with it on both menu bars. Test at the module's end: `make_your_first_song_is_first_in_help`.
- `web/src/lib/guide.ts` (new): `STEPS` (D3's words, with D14's: "double-click"; keys by `chord()` and `opt()`),
  `seen(graph)` (a track playing a Drums-group instrument; 4 notes or more in its lanes; a track playing
  `std:synth-bass` or `std:synth-sub`; 3 notes or more), `openGuide`, `closeGuide`, `foldGuide`, `guideDid("sound")`,
  `useGuide`. Kept with `setLocal("guide", {open, folded, done})` (`folded` beside the plan's two). It ticks for as
  long as the page is open: the store's graph as it changes, the clock every 250 ms (Listen: played while the guide
  is open), and `on("export")` for this song's export (no edit to `exporting.ts`). A song opened with its guide open
  shows the browser.
- `web/src/components/Guide.tsx` (new): the card, 240 px (the browser's, less its 8 px margins), `raised`, 1 px
  `line`, `rounded-md`; the current step `fg` 600 with its hint, done `fg-muted` with a `CircleCheck` in `accent`, the
  rest `fg-faint`; the fold and × (tooltips); the done line; the foot, "Start from an empty song…".
- `Browser.tsx`: `<Guide />` first in the column. `StartScreen.tsx`: the link (and the New Song button calls
  `openNewSong()` with no event). `App.tsx`: `case "guide"` at the switch's end, and its import.
  `NewSongDialog.tsx` (not in the plan's list; nobody else's): `openNewSong({template, after})`, the dialog starting
  on that template and running `after` once the new song opens. `Knob.tsx`: `guideDid("sound")` after a knob's
  commit. `lib/instrument.ts` (where `changeInstrument` lives, not `Tracks.tsx`): `guideDid("sound")` after a change.
- `e2e/st-guide.mjs` (new, kind `none`, 36 checks), its `run.mjs` row after `start`'s `empty` row, its README row.
  `st-tips` gains the guide's card (an app page's Help ▸ Make Your First Song): 45.

**Tests.** `st-guide` 36 of 36: an Empty song through the API; on an app page, `__menu("guide")` shows the browser
and the card first in it, seven steps, none ticked, the first current with "Press ⌘T and pick Drum kit.", kept in
`view.json`; then a page does each step as a person does (⌘T Drum kit; a clip and four notes by double-click; ⌘T
Synth bass; three notes; Space; a Device knob dragged; ⇧⌘E, with the Export dialog's Return when lane C's is
there), each step ticking; the done line; `view.json` keeps seven; a reload keeps them; the fold ("Your first song ·
7 of 7", kept) and back; × closes, its ticks kept; the start screen's link opens New Song on Empty, then the new
song with the card, nothing ticked; the card never covers the timeline; a PC page says "Ctrl+T"; the taste and
tooltip lints on the card three times and on the start screen. Rust: `make_your_first_song_is_first_in_help`,
`the_guide_is_kept_per_song`; `cargo test -p music2-desktop` 18, its clippy and the mac lint clean; `m2-server`'s
clippy clean. Moved: `start` 24, `newsong` 71, `pl-projects` 66, `taste` 39, `s5-knobs` 120, `gestures` 30,
`s8-tracks` 75, `s8-mix` 80, `pl-sounds` 45: all ok (`pl-sounds` failed once in a batch beside another, at "one
revision, and song.ts as it was", which reads the revision right after the file; it passed twice alone).

**Red first.** `st-guide` on `dist-before` and the server at `85b069a`: 2 of 36, then it stops:

```
FAIL  Help ▸ Make Your First Song shows the browser, the guide at its top
FAIL  seven steps, none ticked, the first the current one, with what to do: undefined
```

`make_your_first_song_is_first_in_help` with Help's rows the other way round: `left: [Item("shortcuts"),
Item("guide")] right: [Item("guide"), Item("shortcuts")]`. `the_guide_is_kept_per_song` with `guide` out of `KEYS`:
`guide isn't a view setting` (the `set_view` unwrap).

**Left.** Show in Finder on the done line only for an export made while this window was open (after a reload the
line has no button). The guide's steps don't follow a song whose drums or bass are written in code by other names
(a custom component). first-song's own drums (`std/instruments`' kick and hat, not in + Track's Drums group) don't tick
Add drums: the guide is made for an empty song.

**The app on `:97`** (`tools/app-shot.sh`, the Linux build at `b5beccc`'s web): Help lists Make Your First Song,
Keyboard Shortcuts, then About music2; picking the first shows the card at the top of the side browser, "1 Add
drums. Press Ctrl+T and pick Drum kit." (shots in `~/m2-runs/sst/scratch/shots/d2/`, for the End's pictures).

**For gaps.md:** #11: Help ▸ Make Your First Song, seven steps that tick themselves, in the side browser.

**Contract:** FMT-20's homes: `view.json` gains `guide: {open, folded, done}`. taste.md §5.3: the Help menu lists Make
Your First Song first, then Keyboard Shortcuts.

### D4. Music basics, up to date

**In short**

| Where | Now it says |
|---|---|
| The top | Help ▸ Make Your First Song, the start screen's link, and that every button says what it does |
| Notes | Double-click (or B) adds a note; play with ⌘K or a MIDI keyboard; Record (R) with a bar of clicks first |
| Notes or recordings | File ▸ Import Audio… (⇧⌘I) or a drag; the waveform; move, trim, fade, volume, backwards |
| Time | The tempo and time signature in the top bar; triplets (⌘3); Quantize, Humanize, Swing; the metronome (K) |
| Key | The Key field, and Notes shading the rows not in it |
| The parts of a song | + Track's sounds by their names; Change instrument… |
| Effects, mixing | The browser's drag and the ⋯ menu; M and S, Mute in song, the meters; Automate and drawing points |
| Stems | File ▸ Export… ▸ Stems |

**What changed.** `docs/music-basics.md` only: every "In music2" line says what the app does now, in the file's own
plain style; the lines that said "not in the app yet" for the key, mute and solo, drawing automation, triplets,
quantize and humanize, the tempo and the meters are gone. Code names stay, after the app's words.

**Tests.** None (a doc). Its links checked by hand: `gaps.md#recording-and-midi`, `#notes`, `#automation`,
`#mixing`, `#effects-and-plugins`.

**Left.** It describes lanes A, B and C's recording, audio import and stems as the plan's Decisions have them
(Q17 to Q19, Q7 to Q12, Q39 and Q42, D20, D34). The End checks those lines against what landed and trims any that
didn't (each lane's "If late").

**For gaps.md:** #11: [Music basics](music-basics.md) is up to date, and points at the guide.

**Contract:** none.

## Merge

The four lanes joined on `studio`, in the plan's order (B, C, A, D), each as a merge commit with `rerere` on:
`24033ec` (B), `807386a` (C), `61c6eaf` (A), `67b2197` (D). `main` had not moved (still `85b069a`), so there was
nothing of it to merge. P0 never landed, so lane A's copies of its pieces are the only ones: nothing to choose.

**In short**

| What | Now |
|---|---|
| Lanes merged | All four, every commit of each: each lane's last commit was green |
| Files both sides changed | Joined by the plan's rules (below). No lane's code was dropped |
| Rust | Every test of the touched crates passes, clippy and the Mac lint are clean |
| Browser suites | All 64 ran. Two failed because of the merge, and each has a fix below |
| Renders | 10 of 10 match `e2e/renders.sha256` |

**How the shared files were joined**

| File | Joined as |
|---|---|
| `crates/server/src/lib.rs`, `routes.rs`, `graph.rs` | Every lane's `mod` and `.merge` lines, after `routes_leases` and `tempo_lease`: A's (`routes_audio`, `peaks`, `import`), then B's (`routes_record`, `record`), then C's (`routes_exchange`, `stems`, `exchange`) |
| `crates/editor/src/lib.rs` | After `pub mod rekey;`: `import` (A), `smf`, `midifile` (C), `explain` (D) |
| `apps/music2-desktop/src/menu.rs` | `ITEMS`: A's `import_audio` after `save_as`, then C's `export` titled "Export…". Every lane's test kept |
| `web/src/App.tsx` | Every lane's imports; the cases at the switch's end: `import_audio` (A), `record` (B), `guide` (D); `<ExportDialog />` (C) and `<DropLayer />` (A) where each lane put it |
| `web/src/components/SongTimeline.tsx` | Every lane's imports; A's audio rows after Split's, C's Export as MIDI… after Repeat… |
| `docs/designs/step8/build-notes-studio.md` | Each lane's own section, as that lane wrote it |
| `e2e/run.mjs`, `e2e/README.md` | Git joined every lane's rows by itself |

**Checks at the merge** (the server and `m2` built from `67b2197`, copied to `bin/st-67b2197`; the page from
the same tree)

| Check | Result | Log (`~/m2-runs/logs/`) |
|---|---|---|
| `cargo build --locked` (server, `m2`); `Cargo.lock` unchanged | ok | `st-int-build-*`, `st-int-build2-*` |
| `cargo test --locked --no-fail-fast` of `m2-song`, `m2-runtime`, `m2-play`, `m2-ir`, `m2-script`, `m2-session`, `m2-editor`, `m2-server`, `music2-desktop`, `m2` | 1,374 passed, 0 failed (editor 378, server 356, desktop 22, and the rest) | `st-int-tests-*` |
| `cargo clippy --locked --all-targets -- -D warnings` of the same crates, and the Mac lint (`aarch64-apple-darwin`) | clean | `st-int-clippy-*` |
| The web build (with its type checks) | ok | — |
| Renders (`baseline.sh --tree`, then `shasum -c e2e/renders.sha256`) | 10 of 10 OK | `st-int-renders-*` |
| All 64 browser suites, in three batches at once (`CLAP_PATH` set for `live3b`) | 62 ok; `st-tips` 44/45 and `pl-notes` 101/102, fixed below | `st-int-suites1-*`, `-2-*`, `-3-*` |

**Fixes the merge needed**

| Suite | What broke | Why | Fix |
|---|---|---|---|
| `st-tips` (D) | "the question before an export with a track muted shows" failed | Lane C's Export dialog now comes before the question | The suite presses Return in the dialog first (Song is chosen), as `fx-fixes` does. Still 45 checks |
| `pl-notes` | "the field keeps its width" failed: the song name's field was 106 px wide, not at least 120 | Lane B's Record button takes 32 px, so the Mac app's room for the name at 1440 px is now 106 px. The suite leaves the Mac layout and starts typing at once, before the page has measured the browser's room again (242 px). Only the suite does this: the app never changes layout | The suite waits 300 ms after leaving the Mac layout, as it does after each width. Still 102 checks |

Red first: `st-tips` 44 of 45 on the merged tree (`st-int-suites1-*`: `FAIL  the question before an export with a
track muted shows`); 45 of 45 with the fix (`st-int-plnotes-*`).
`pl-notes` 101 of 102 on the merged tree and on lane B's page alone (`st-int-plnotes-*`, `st-int-plnotes-lanes-*`:
`FAIL  the field keeps its width, on top of its neighbours: {"w":106,"top":true} then {"w":106,"top":true}`), and 102
of 102 on lane A's page; with the fix, 102 of 102 (`st-int-plnotes2-*`).

**A flaky suite, not the merge's:** `pl-notes` sometimes stops at a check the merge didn't touch ("the focus is back
in the roll…", or a note not found right after Esc) when several suites run at once. Run four at a time, the page from
`85b069a` failed 2 of 4 and the merged page 2 of 4 (`st-int-plnotes-flake-*`). Alone, with the fix, the merged page
passed 2 of 3 (`st-int-plnotes2-*`), and 4 of 4 more in a copy that only logged more. For the End: run `pl-notes`
alone, and again if it stops at one of those two.

**Clean-up:** the lane worktrees `wt/st-a` to `wt/st-d` are removed and their branches deleted (each was merged; its
commits are in `studio`'s history). The lanes had already deleted their targets and `bin/` copies. Kept for the End:
`target-st` (36 GB), `bin/st-67b2197`, `scratch/dist-before` (the page at `85b069a`, for red-first runs), and the
lanes' pictures in `scratch/` (`a-app1` to `a-app8`, `a-shots`, `c-shots`, `shots/d2`).

**Left for the End** (from the lanes' reports): Q30 (no export while recording); `tips()` in `st-audio`,
`st-record`, `st-midi` and `st-stems`, and `st-tips`' joined places (the Record button and its menu, the take, the
Export dialog, the volume popover, the drop hint); C2b (a MIDI file as new tracks, Import MIDI…, `.mid` drops,
the tempo question), which can now use lane A's `tracks::add_tracks` and `drop.ts`; Export as MIDI… in the app needs
a `save_file` action (it works in a browser page); the docs (gaps.md, README, architecture, taste.md, HANDOFF) from
each section's For gaps.md and Contract lines; `music-basics.md` names no MIDI import, so it needs no trim.

## End

The lead's last checks, docs and pictures.

### The review's fixes

The End's review confirmed 12 findings (R1 to R7, R9, F1 to F3, F6; R3 and F2 are one). Each is fixed in a commit of its own, with
a test that failed first. Red runs: the Rust tests in `~/m2-runs/logs/st-end-red1-*`, the browser checks on the
page and server from `37a6936` (`bin/st-67b2197`, the same Rust) in `st-end-e2e-red-*`.

#### R1. Recording while the code doesn't compile

A take is written as an edit, and no edit can be made while the code doesn't compile. Record never checked, and a take
that ended then was thrown away with "Bass is gone".

- `record.rs`: `record_start` refuses with lockReason's words, "Fix the code first: it doesn't compile right now."
  A take that ends while the code is broken waits in the queue: the windows hear once "The recording waits for the
  code. Fix the code, and it goes on Bass.", and the take thread writes it once the code compiles. "Bass is gone"
  is said only when the lane is gone; read-only and a closed song say so.
- `clips.rs`: `clips_why` says why the editor refused (`Refused`); `clips_as` is built on it, as before.
- `lib/record.ts`: R, the button and the menu say lockReason's words and start nothing; `waiting` is a toast.
- Tests: `record_is_refused_while_the_code_doesnt_compile`,
  `a_take_that_ends_while_the_code_is_broken_waits_for_the_code`; `st-record` gains 2 checks (61 to 63).
- Red: `called Result::unwrap_err() on an Ok value: … "phase": String("count")`; `[… {"error": String("The recording
  couldn't be kept: Bass is gone.")}]`; `FAIL  with the code broken, R says to fix it first ([])`.
- Left: closing the song while a take waits for the code loses it (said: "its song was closed").

#### R2. The loop changed while a take records

The take's loop end was read once, at R. A loop turned on mid-take put notes after the wrap past the loop's end; one
turned off still ended the take at the old end.

- `record.rs`' `record_loop`, called by one line in `Graph::set_loop`: the take's end becomes the new loop's end
  while the song is still before it (a song past it plays on, as the engine does), or none with no loop.
- Tests: `a_loop_turned_on_during_a_take_ends_it_at_the_loops_end`, `a_loop_turned_off_during_a_take_lets_it_go_on`.
- Red: both `no take was written` (the first never ended; the second ended empty at the old end, before the note).
