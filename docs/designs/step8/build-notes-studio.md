# Studio: build notes

What each part of the studio track built, its tests, the red run before each change, and what is
left. The plan is `~/m2-runs/sst/plan.md`. Each part writes only under its own heading.

## P0

The lead's ground: written by the lead.

## Lane A

Audio in the app: import, audio tracks, waveforms, and editing audio clips. Branch `studio-a`.

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

## Lane B

Record what you play: written by lane B.

## Lane C

MIDI files and stems: written by lane C.

## Lane D

Plain-language help: written by lane D.

## Merge

The lead's merges.

## End

The lead's End.
