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
