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
