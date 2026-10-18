# Track sx: build notes

Branch `sx`, from `main` at `97b4cdf`, 2026-10-18. The plan is [sx-plan.md](sx-plan.md); what changed while building is written there, under each slice.

## In short

- **Slice 1, kernel ABI v1 settled:** music2 reads a module's ABI version from its bytes first. A kernel may read its audio file from the disk.
- **Slice 2, `std:stretch`:** warped clips are stretched by a std kernel now, not by the engine. It is the first **offline kernel**: music2 runs it off the audio thread, in pieces. Neither the file nor the stretch is ever whole in memory.
- One example sounds a little different: `examples/resample` (its 2 renders). The other 10 renders keep their bits.
- Linux timings are information only. The Mac's run is owed (PLAT-1).

## Slice 2, piece by piece

| Piece | Where | Test (each red first) |
|---|---|---|
| The kernel: WSOLA, channels aligned together, the match normalized by loudness, pitch by cubic Hermite | `std/kernels-src/src/stretch.rs` | `a_stretch_keeps_its_pitch`, `a_stretch_is_as_long_as_its_map_says`, `the_identity_map_gives_the_source_back`, `a_stretch_has_no_clicks`, `pitch_with_a_map_transposes_and_keeps_time` |
| The kit: `Cfg::offline()`, `Cfg::time_map()` | `std/kernels-src/kernel/src/lib.rs` | the kernel's tests use them |
| Offline kernels in the manifest; song code gets `kernel.offline` | `core/runtime/src/packages.rs`, `core/ir/src/check.rs` | `song_code_cannot_call_an_offline_kernel` |
| A run: one sandbox, `flags` bit 0 and the time map in its settings, block by block into a sink | `core/runtime/src/offline.rs`, `sandbox.rs` | `a_warped_clip_is_stretched_by_std_stretch` |
| The cache: the time map from the markers and the tempo; the stretch written in pieces; gain, fades and reverse as it is read | `core/runtime/src/audio.rs`, `stream.rs` | `a_warped_render_holds_neither_file_whole`, `an_export_with_an_empty_cache_folder_equals_one_with_a_full_one`, `a_streamed_lane_is_the_rendered_lane` |
| The example on many threads | `core/play/tests/bit_identity.rs` | `the_resample_example_gives_the_same_bits_on_1_to_7_threads` |

The engine's own WSOLA and `WARP_VERSION` are gone. A stretch's file is named by all the kernel is given, std's module hash among them.

## Renders (FMT-3)

| Render | Changed? | Why |
|---|---|---|
| `examples-resample.wav`, `examples-resample-from4-for6.wav` | yes | its 2 warped clips are stretched by std 0.2.2's kernel: the time map as pairs, the match normalized by loudness, and no fade-in at a stretch's start. std went from 0.2.1 to 0.2.2 |
| the other 10 | no | |

## Speed (Linux, information only)

| What | Time |
|---|---|
| 60 s of stereo 44.1 kHz, stretched to 75 s and up 3 semitones, into a 48 kHz song, written to its file | 0.25 s: about 300 times faster than real time (`stretch_speed_report`) |

## A check by ear

About 2 minutes. No test can do it, as the tests play no sound.

| # | Do | You should hear |
|---|---|---|
| 88 | Click **Open an Example**, then **Resample**. Press Space. Listen from 3 seconds in (bar 2, beat 3) to 5 seconds, then from 6 seconds (bar 4). | At 3 s the keys come back slower, stretched over 4 beats. Their pitch is the same as at the start. No wobble, no clicks, no echo. At 6 s they come back higher and stretched, still on the beat. |
| 89 | Tell the lead what you heard at 88. | |
